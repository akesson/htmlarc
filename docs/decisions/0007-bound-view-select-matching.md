# 0007 — Bind one DomView per select walk (view-based matching)

- **Status:** Accepted — implemented on branch `perf/c1-bound-view-select` and measured (see
  [Consequences](#consequences)). Follows [PR #38] (the `verify` combinator-free fast path), which
  this builds on directly.
- **Date:** 2026-06-20
- **Scope:** `crates/htmlarc-dom` (`DomRead` trait, `iters/match_iter.rs`, `css/selectors/*`,
  `dom/dom_view.rs`); no format change, no `crates/htmlarc-archive` format work (only the
  `Doc` `DomRead` impl gains one method).
- **Companion:** the second select-walk lever found in the 2026-06-20 profiling pass; the first
  (skip the recursive `verify` trampoline) shipped as PR #38.

## Context

After PR #38, the warm CSS-select walk over a **memory-mapped** document stands at ~1.50× scraper
(owned ~1.18×, with owned `tag` already *faster* than scraper at 0.87×). samply self-time on the
mmap path now has one dominant, mmap-specific bucket:

- `rkyv::vec::ArchivedVec::len` **~24%** + `ArchivedNodes::view` **~6%** ≈ **30%** of self-time.

This is the **per-accessor view rebuild**. Every per-node match primitive re-derefs the archived
blob to reconstruct its sub-views:

- `MatchIter::next` calls `element.tag()` → `Doc::with_nodes` → `with_dom` → `ArchivedNodes::view()`
  (one `ArchivedVec` deref: `len` + relative-ptr resolve), then
- `selectors.matches(&element)` → `compound::matches` → `el.has_class_selectors()` /
  `has_id_selector()` / `has_attributes()`, each → `Doc::with_view` → `with_dom` →
  `ArchivedDomInner::view_with` → **six** `*.view()` sub-derefs (`nodes`, `attrs`, `symbols`,
  `class_lists`, `ext_tags`, `strings`).

hotpath confirms the call structure exactly: per node there are **2 `with_dom` rebinds** (one for
`tag()`, one for the class/attr check), and `with_view` builds the full six-sub-view `DomView` even
though a class check needs only three. The `with_dom`/`LazyState`/`bind` work itself is cheap
(6–14 ns); the cost is the **repeated `ArchivedVec` dereferencing** (~7 per node) to rebuild views
that are identical for every node in the walk.

The **owned** path (`DomInner`, a plain `Vec<u8>`) has **zero** `ArchivedVec::len` — a `Vec` deref
is a field read. So the post-PR-38 mmap-vs-owned gap (tag 1.11× vs 0.87×, class 1.51× vs 1.20×,
**mmap +27–33%**) is almost entirely this rebuild, and the owned path is the measured ceiling for
removing it.

The blob does not change during a `&self` select, so rebuilding its sub-views per accessor is pure
waste. The fix is to build the view **once per walk** and reuse it.

### Why it isn't already done

`DomRead::with_view`/`with_nodes` are **closure-scoped** (`fn with_view<F: FnOnce(DomView)>(…)`)
specifically so the mutable `DomRefCell` can scope its `RefCell` borrow guard to the call
(`f(self.dom.borrow().view())`, `dom_wrappers.rs:172`). And `Doc::with_dom` rebuilds the
`ArchivedDom` transiently to avoid a self-referential field on `Doc` (the `LazyState` lives only for
the call). Both are correct reasons the *current* API rebuilds; neither prevents an *immutable*
backing from handing out a view bound to the whole borrow — `DomRef::dom_view()` already does
exactly that (`-> DomView<'_>` tied to `&self`), and is implemented by every backing except
`DomRefCell`.

## Decision

Bind one `DomView` for the whole walk on immutable backings and match every node against it,
replacing the per-accessor rebuild with a per-walk one.

### 1. `DomRead::walk_view` — a per-walk bound view (opt-in, immutable backings only)

Add a provided method to `DomRead`:

```rust
/// A view bound to the full borrow, reused across every node of a walk so per-node matching does
/// not rebuild the (rkyv) sub-views per accessor. Returns `None` for backings whose view is a
/// transient borrow that cannot be held across iteration (`DomRefCell`'s scoped `RefCell` guard);
/// such backings keep the per-call closure path. Topology/attribute matching after `resolve` is
/// integer-only, so the bound view needs no live text source.
fn walk_view(&self) -> Option<DomView<'_>> { None }
```

Overrides:

- `DomInner` → `Some(self.view())` (the same value `dom_view()` returns).
- `Doc` (mmap) → `Some(self.entry.bind(StringSource::plain(&[])).view())` — **non-inflating**.
  `entry.bind` ties the `ArchivedDom` to the archive lifetime `'a` (which outlives `&self`), and
  `ArchivedDom::view()` returns `DomView<'a>` (not `&self`-scoped). The **empty** `StringSource`
  is sound because: (a) `resolve` binds class/id/attr names against the **symbol table**, a
  separate store that never touches the relocated text pool, and (b) resolved per-node matching is
  pure integer comparison. Neither reads `StringSource`, so no zstd inflation happens (today's
  `Doc::dom_view()` force-inflates — `walk_view` must *not* use it).
- `DomRefCell` → keeps the default `None`.

### 2. View-based matching: `matches_in_view(view, index)`

Thread the bound view + node index through the selector match, mirroring the existing element-based
path but calling the `DomView` node-indexed primitives that already exist
(`dom_view.rs`: `has_class_selectors(node, …)`, `has_id_selector(node, …)`, `has_attributes(node,
…)`, `has_classes(node, …)`, `nodes.tag(node)`, `nodes.tag_byte(node)`):

- `SelectorList::matches_in_view(&self, view, index) -> bool` — any complex matches.
- `ComplexSelector::matches_in_view`: **combinator-free → `self.first.matches_in_view(view, index)`**
  (the PR #38 fast path, now view-based). With combinators → fall back to the element path
  (build `HtmlElement::new(dom, index)`, call the existing `self.matches(el)`); combinators are
  rare, off the hot path, and `verify`/`RelativeIter` already navigate via elements.
- `CompoundSelector::matches_in_view(view, index)`: the `matches()` checks, but against `view`:
  tag → `view.nodes.tag(index)`; ext-tag → view tag-name check; id → `view.has_id_selector`;
  attrs → `view.has_attributes`; classes → `view.has_class_selectors`; class-attrs →
  `view.has_classes`. **If the compound has a `text` pattern or any `pseudo_classes`, fall back to
  the element path** — text matching reads strings (the bound view is empty) and pseudo-classes
  navigate via elements. Both are uncommon and none of the eight bench queries hit them.

### 3. `MatchIter` dispatch

`MatchIter::new` caches `bound: Option<DomView<'dom>>` = `iter.dom().walk_view()`. `MatchIter::next`
branches once on it (the `Option` is set at construction, so the branch is trivially predicted):

```rust
fn next(&mut self) -> Option<HtmlElement<'dom, Dom>> {
    let dom = self.iter.dom();
    if let Some(view) = &self.bound {                    // immutable backings (DomInner, Doc)
        while let Some(idx) = self.iter.next_index() {
            if view.nodes.tag(idx) == HtmlTag::sys_text { continue; }
            if self.selectors.matches_in_view(view, idx) {
                return Some(HtmlElement::new(dom, idx));
            }
        }
        None
    } else {                                             // DomRefCell: unchanged element path
        while let Some(idx) = self.iter.next_index() {
            let el = HtmlElement::new(dom, idx);
            if el.tag() == HtmlTag::sys_text { continue; }
            if self.selectors.matches(&el) { return Some(el); }
        }
        None
    }
}
```

Borrow soundness: the bound `DomView<'dom>` borrows the doc (`'dom`), not `self`; `iter.next_index()`
borrows `&mut self.iter` and (for `LinearSweep`) touches no dom state — it is a pure counter that
reads no link bytes. All doc borrows are shared `&`, so holding the view across the walk is sound.
`HtmlElement::new` is two words, no view work; it is built only for returned matches.

## Consequences

- **Performance (MEASURED — exceeds the projected ceiling).** Criterion same-session A/B vs a
  `before` baseline at PR #38, fixture `fr.serrer.html`, scraper measured in the same run (its
  drift stayed within ±3.5% → noise, so the scraper-normalized ratio below is the drift-immune
  headline). **All eight queries kept exact match-count parity with scraper.**

  | query            | mmap before× | mmap after× | mmap Δ | owned before× | owned after× | owned Δ |
  |------------------|-------------:|------------:|-------:|--------------:|-------------:|--------:|
  | tag `div`        | 1.12         | **0.85**    | −23.6% | 0.85          | **0.81**     | −5.2%   |
  | class present    | 1.49         | **0.96**    | −34.7% | 1.20          | **0.96**     | −20.2%  |
  | class absent     | 1.68         | **0.98**    | −40.7% | 1.32          | **0.98**     | −26.6%  |
  | multi-class      | 1.48         | **0.93**    | −36.1% | 1.20          | **0.96**     | −20.5%  |
  | id               | 1.60         | **1.10**    | −30.3% | 1.32          | **1.09**     | −17.5%  |
  | attr exact       | 1.39         | **0.93**    | −32.0% | 1.17          | **0.93**     | −22.4%  |
  | attr insensitive | 1.40         | **0.96**    | −31.5% | 1.20          | **0.95**     | −20.6%  |
  | attr presence    | 1.39         | **0.93**    | −32.5% | 1.18          | **0.94**     | −21.6%  |

  **mmap went ~1.44× → ~0.96× average; owned ~1.18× → ~0.95×.** Both paths now **beat scraper on
  7 of 8 queries** (only `#id` stays above, ~1.1×, where scraper's id index wins), and the
  mmap↔owned gap has closed (both ~0.95× — within noise). htmlarc is now *faster* than scraper on
  CSS `.select()` for these shapes, on both the memory-mapped and owned read paths.

  The win is **larger than the ~1.18× ceiling the design projected.** That projection counted only
  the ~30% `ArchivedVec` rebuild bucket; it under-counted because binding the view once *also*
  eliminated, per text node visited, (a) the `HtmlElement` construction and (b) the `element.tag()`
  skip-text accessor — which on mmap was itself a full `with_dom` rebuild. Text nodes are a large
  fraction of every walk and *dominate* whole-tree scans, which is why the no-match `class absent`
  scan improved most (−40.7%): it visits every node, and a text node now costs one integer
  `view.nodes.tag()` read instead of an element + view rebuild. The "ceiling = owned" model was
  therefore a lower bound, because the change improved owned too (owned `class absent` −26.6%).

- **Mechanism confirmed (hotpath, mmap `class-present`).** `mmap::with_dom` dropped from a
  per-node-per-check rebuild to **2 calls per `select`** (the one-time `resolve` view + iterator
  setup), 0.23% of self-time (was ~30% across `with_dom`/`view_with`/`ArchivedVec::len`).
  `match_iter::next` now holds ~93% — the matching work itself, as intended.
- **No format/API-surface change.** `walk_view` is an internal provided method; `matches_in_view`
  is `pub(crate)`. The public `.select()` signature and behaviour are unchanged.
- **Correctness model.** The fast path runs only when the bound view is present (immutable backing)
  *and* the compound is combinator/text/pseudo-free; everything else falls back to the existing,
  unchanged element path. Match results must be identical — guarded by the `select_vs_scraper`
  parity assert and the existing selector test suite (which already covers single-compound,
  combinator, `:not/:is/:has`, id, attr, and direct-`matches_css` cases).

## Alternatives considered

- **Cache the bound view on `Doc`/`DomInner` (e.g. `OnceCell<DomView>`).** Self-referential: the
  view borrows the backing it would be stored in. This is the exact reason `with_dom` rebuilds
  transiently. Rejected.
- **Give `HtmlElement` a borrowed `&DomView` field.** Would remove the rebuild too, but changes the
  size and construction of the most pervasive type in the crate and ripples through every accessor
  and iterator. Higher churn, larger blast radius. Rejected in favour of the contained
  `matches_in_view` path.
- **Build only the three needed sub-views per node (drop `attrs`/`ext_tags`/`strings` for class
  checks).** A smaller win (cuts the per-node deref count from ~7 to ~4 but still per-node) and
  messier (a partial-view type). Subsumed by binding the full view once.

## Verification (done)

1. ✅ `cargo test --workspace` green (410 tests); `cargo clippy --workspace --all-targets` clean. The
   `select_vs_scraper` / `selectors_vs_scraper` parity prints show identical match counts to scraper
   on all eight queries.
2. ✅ hotpath (`profile_walk --mode mmap --query class-present`): `mmap::with_dom` collapsed from a
   per-node rebuild to 2 calls/`select` (0.23% self-time); the per-accessor view rebuild is gone.
3. ✅ Same-session criterion A/B vs a `before` baseline at PR #38 on both benches — results in
   [Consequences](#consequences). The measured win (mmap −24–41%, landing ~0.96× scraper) *exceeds*
   the projected ~1.18× ceiling, for the reason analysed there; scraper drift stayed ≤3.5% (noise),
   so the scraper-normalized ratio is the headline.
4. ✅ `DomRefCell` mutate-while-iterating tests unchanged and green — it keeps `walk_view() == None`
   and stays on the per-call element path.

[PR #38]: https://github.com/akesson/htmlarc/pull/38
