use super::{RunIndex, RunVec, TERMINATOR};

/// Repackage-path compaction for a [`RunVec`]: mark each node-referenced run, then copy
/// only those into a fresh arena — dropping mutation garbage and unreferenced runs —
/// while remapping values to dense new ids.
///
/// `value_reidx` numbers the used values in ascending old-id order, which is exactly the
/// order `SymbolTable::rebuilt` re-inserts the surviving strings in: the two stay coupled
/// without any cross-talk (pinned by `rebuilt_drops_unused_and_matches_rebuilder_numbering`).
pub(crate) struct RunRebuilder {
    /// Indexed by old start offset; `Some` marks a run referenced by a live node, and
    /// [`build`](Self::build) replaces the mark with the run's new start offset.
    runs_reidx: Vec<Option<u16>>,
    /// Indexed by old value; `Some` marks a value used by a live run, and
    /// [`build`](Self::build) replaces the mark with its dense new id.
    value_reidx: Vec<Option<u16>>,
}

impl RunRebuilder {
    pub fn new(arena_len: usize, value_count: usize) -> Self {
        Self {
            runs_reidx: vec![None; arena_len],
            value_reidx: vec![None; value_count],
        }
    }

    pub fn mark_run_used(&mut self, runs: &RunVec, start: RunIndex) {
        self.runs_reidx[start.as_usize()] = Some(0);
        for value in runs.run_at(start) {
            self.value_reidx[value as usize] = Some(0);
        }
    }

    pub fn build(self, runs: &RunVec) -> RunsRebuilt {
        let Self {
            mut runs_reidx,
            mut value_reidx,
        } = self;
        for (index, val) in value_reidx.iter_mut().flatten().enumerate() {
            *val = index as u16;
        }

        let mut rebuilt = RunVec::with_capacity_as(runs);
        for (old_start, slot) in runs_reidx.iter_mut().enumerate() {
            if slot.is_none() {
                continue;
            }
            let new_start = rebuilt.arena.len() as u16;
            for value in runs.run_at(RunIndex(old_start as u16)) {
                let new_value =
                    value_reidx[value as usize].expect("all run values must be reindexed");
                rebuilt.arena.push(new_value);
            }
            rebuilt.arena.push(TERMINATOR);
            *slot = Some(new_start);
        }
        RunsRebuilt {
            runs_reidx,
            value_reidx,
            runs: rebuilt,
        }
    }
}

pub(crate) struct RunsRebuilt {
    pub(crate) runs_reidx: Vec<Option<u16>>,
    pub(crate) value_reidx: Vec<Option<u16>>,
    pub(crate) runs: RunVec,
}
