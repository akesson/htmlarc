use rkyv::rancor::Error;

use crate::html::HtmlAttr;
use crate::stores::RunRebuilder;
use crate::stores::symbols::{SymbolTable, SymbolTableBuilder};

use super::builder::AttrStoreBuilder;
use super::store::ArchivedAttrStore;
use super::{AttrName, NAME_EXT_BASE};

/// A symbol table holding `names` as extended attribute names; returns it plus each name's
/// `NameSym` (sym + bias). Lets tests deref extended names the way `DomInner` does.
fn ext_names(names: &[&str]) -> (SymbolTable, Vec<u16>) {
    let mut b = SymbolTableBuilder::default();
    let syms: Vec<u16> = names
        .iter()
        .map(|n| b.intern_or_poison(n).as_u16() + NAME_EXT_BASE)
        .collect();
    (b.build(), syms)
}

#[test]
fn build_dedups_pairs_and_sorts_numerically() {
    let mut b = AttrStoreBuilder::default();
    let href = HtmlAttr::href as u16;
    let id = HtmlAttr::id as u16;
    // Element with href="/x", id="a"; a second href="/x" dedups to the same entry.
    let run = b.new_run(href, "/x");
    b.append_last(run, id, "a");
    b.append_last(run, href, "/x"); // duplicate pair -> no new entry, deduped in run
    let store = b.build();
    let view = store.view();

    assert_eq!(view.entry_count(), 2, "distinct (name, value) pairs");
    // The run holds two entries (the duplicate href was dropped by the run's own dedup).
    assert_eq!(store.lists.run_at(run).count(), 2);

    // find_entry resolves a pair to its stable id without touching strings.
    let vref = view.value_ref("/x").unwrap();
    let entry = view.find_entry((href, vref)).unwrap();
    assert_eq!(view.value_at(entry), "/x");
    assert_eq!(view.name_sym(entry), href);
    assert_eq!(view.value_ref("missing"), None);
}

#[test]
fn extended_names_decode_through_symbols() {
    let (symbols, syms) = ext_names(&["data-mw", "tabindex"]);
    let mut b = AttrStoreBuilder::default();
    let run = b.new_run(syms[0], "interface");
    b.append_last(run, syms[1], "0");
    let store = b.build();
    let view = store.view();
    let sym_view = symbols.view();

    let got: Vec<String> = store
        .lists
        .run_at(run)
        .map(|id| view.attribute_at(id, sym_view).to_string())
        .collect();
    assert_eq!(got, vec!["data-mw=\"interface\"", "tabindex=\"0\""]);
    assert!(matches!(
        view.attribute_at(
            view.find_entry((syms[0], view.value_ref("interface").unwrap()))
                .unwrap(),
            sym_view
        )
        .name,
        AttrName::Ext("data-mw")
    ));
}

#[test]
fn archived_round_trip() {
    let mut b = AttrStoreBuilder::default();
    let run = b.new_run(HtmlAttr::id as u16, "a");
    b.append_last(run, HtmlAttr::href as u16, "/x");
    let store = b.build();

    let bytes = rkyv::to_bytes::<Error>(&store).unwrap();
    let archived = rkyv::access::<ArchivedAttrStore, Error>(&bytes[..]).unwrap();
    let view = archived.view();

    let id = HtmlAttr::id as u16;
    let vref = view.value_ref("a").unwrap();
    let entry = view.find_entry((id, vref)).unwrap();
    assert_eq!(view.value_at(entry), "a");
    assert_eq!(view.run_at(run).count(), 2);
}

#[test]
fn rebuilt_compacts_and_remaps_extended_names() {
    // Two elements: one keeps a std + ext attribute, the other (dropped) holds a different
    // ext name. Rebuild must drop the unused entry/value AND remap the surviving ext name
    // through the symbol reindex.
    let (_symbols, syms) = ext_names(&["data-keep", "data-drop"]);
    let keep = syms[0];
    let drop = syms[1];

    let mut b = AttrStoreBuilder::default();
    let r_keep = b.new_run(HtmlAttr::id as u16, "x"); // entry 0
    b.append_last(r_keep, keep, "v"); // entry 1
    let _r_drop = b.new_run(drop, "gone"); // entry 2 (will be dropped)
    let store = b.build();

    // Mark only r_keep as live.
    let mut rb = RunRebuilder::new(store.lists.len(), store.entries.len());
    rb.mark_run_used(&store.lists, r_keep);
    // Symbol reindex: "data-keep" (sym 0) survives -> new id 0; "data-drop" dropped.
    let sym_reidx = vec![Some(0u16), None];
    let rebuilt_runs_src = rb.build(&store.lists);
    let attrs = store.rebuilt(
        &rebuilt_runs_src.value_reidx,
        &sym_reidx,
        rebuilt_runs_src.runs,
    );

    // Only the two kept entries survive; the dropped ext name's entry is gone.
    assert_eq!(attrs.entries.len(), 2);
    // The surviving ext name remapped to (new sym 0) + bias.
    let view = attrs.view();
    let new_run = rebuilt_runs_src.runs_reidx[r_keep.as_usize()].unwrap();
    let names: Vec<u16> = view
        .run_at(new_run.into())
        .map(|id| view.name_sym(id))
        .collect();
    assert_eq!(names, vec![HtmlAttr::id as u16, NAME_EXT_BASE]); // id, data-keep@new-sym-0
    let _ = drop;
}
