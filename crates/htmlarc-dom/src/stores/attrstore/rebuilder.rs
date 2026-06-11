use crate::stores::RunVec;

use super::{AttrStore, NAME_EXT_BASE};

impl AttrStore {
    /// Repackage-path compaction (ADR 0002 §3). Driven from [`DomInner::rebuild`], which
    /// has already:
    ///   1. marked the live attribute runs (so `entry_reidx` dense-numbers the surviving
    ///      entry ids in ascending old-id order, and `rebuilt_runs` holds the runs already
    ///      remapped to those new ids), and
    ///   2. compacted the document symbol table, yielding `sym_reidx` — into which the
    ///      *extended attribute names* were marked alongside the class tokens (they share
    ///      that table). Without that union the names would be dropped here and every
    ///      extended `NameSym` would dangle.
    ///
    /// This compacts the store's own value table the same way and re-emits the surviving
    /// entries with remapped names and value refs. Because every renumbering is componentwise
    /// strictly monotone (std names keep identity; extended names and value refs are dense
    /// ascending), the old numeric permutation filtered through `entry_reidx` stays sorted —
    /// no re-sort (asserted below; mirrors `SymbolTable::rebuilt`).
    pub(crate) fn rebuilt(
        &self,
        entry_reidx: &[Option<u16>],
        sym_reidx: &[Option<u16>],
        rebuilt_runs: RunVec,
    ) -> AttrStore {
        // 1. Mark every value ref used by a surviving entry, then dense-number them ascending
        //    — the order `SymbolTable::rebuilt` re-inserts the strings in.
        let mut val_reidx: Vec<Option<u16>> = vec![None; self.values.len() as usize];
        for (old_id, slot) in entry_reidx.iter().enumerate() {
            if slot.is_some() {
                let (_, vref) = self.entries[old_id];
                val_reidx[vref as usize] = Some(0);
            }
        }
        for (next, slot) in val_reidx.iter_mut().flatten().enumerate() {
            *slot = next as u16;
        }
        let values = self.values.rebuilt(&val_reidx);

        // 2. Re-emit surviving entries in ascending old-id order (matches `entry_reidx`'s
        //    dense numbering), remapping the name sym (identity for std, shifted for ext) and
        //    the value ref.
        let mut entries: Vec<(u16, u16)> = Vec::with_capacity(values.len() as usize);
        for (old_id, slot) in entry_reidx.iter().enumerate() {
            if slot.is_none() {
                continue;
            }
            let (name, vref) = self.entries[old_id];
            let new_name = if name < NAME_EXT_BASE {
                name
            } else {
                sym_reidx[(name - NAME_EXT_BASE) as usize]
                    .expect("a live extended attribute name must be marked in the symbol rebuild")
                    + NAME_EXT_BASE
            };
            let new_vref = val_reidx[vref as usize].expect("a used value ref must be reindexed");
            entries.push((new_name, new_vref));
        }

        // 3. Filter the old numeric permutation through the entry reindex — stays sorted.
        let sorted: Vec<u16> = self
            .sorted
            .iter()
            .filter_map(|&old| entry_reidx[old as usize])
            .collect();

        let rebuilt = AttrStore::from_parts(values, entries, sorted, rebuilt_runs);
        debug_assert!(
            rebuilt.is_numerically_sorted(),
            "rebuilt attribute permutation must stay numerically sorted"
        );
        rebuilt
    }

    // Always compiled (not `#[cfg(debug_assertions)]`): it is called inside `debug_assert!`,
    // whose argument compiles in release too (the PR 2.5 lesson). Optimized out when debug
    // assertions are off.
    fn is_numerically_sorted(&self) -> bool {
        self.sorted
            .windows(2)
            .all(|w| self.entries[w[0] as usize] <= self.entries[w[1] as usize])
    }
}
