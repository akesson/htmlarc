mod rebuilder;

#[cfg(test)]
mod tests;

use std::fmt::Display;

pub(crate) use rebuilder::RunRebuilder;
use rkyv::{Archive, Deserialize, Serialize};

/// Start offset of a run in a [`RunVec`] arena. Stored in a node slot where `0xFFFF` is
/// the "no list" sentinel, so starts must stay `<= 0xFFFE` (guaranteed by [`ARENA_CAP`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunIndex(u16);

impl RunIndex {
    pub fn as_u16(&self) -> u16 {
        self.0
    }
    pub fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

impl Display for RunIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for RunIndex {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// Ends each run in the arena. It coincides with the node-slot "no list" sentinel, so
/// stored values must stay below it — [`Sym`](super::Sym)s do (`LOCAL_CAP = 0xEF00`).
const TERMINATOR: u16 = u16::MAX;

/// Maximum arena slots (values + one terminator per run): every slot must be addressable
/// by a `u16` offset distinct from the `0xFFFF` sentinel. The per-document escalation to
/// u24 slots for the docs this cannot hold is ADR 0002 PR 6.
const ARENA_CAP: usize = u16::MAX as usize;

const OVERFLOW_MSG: &str = "RunVec overflow: more than 65,535 list entries in one document";

/// Contiguous runs of `u16` values in one append-only arena (ADR 0002, list storage plan).
///
/// Each run is a consecutive slice of values followed by a [`TERMINATOR`], addressed by
/// its start offset ([`RunIndex`]) — no list table and no per-entry next-pointer, so an
/// entry costs 2 bytes against the linked `ListVec`'s 4. Runs are written whole at parse
/// time. Live mutation appends in place only for the trailing run; any other run is
/// relocated to the arena end and its old slots become unreachable garbage until the
/// document repackage compacts them (the same GC point that compacts the symbol table).
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct RunVec {
    arena: Vec<u16>,
}

impl RunVec {
    pub fn with_capacity_as(other: &Self) -> Self {
        Self {
            arena: Vec::with_capacity(other.arena.len()),
        }
    }

    pub(crate) fn view(&self) -> RunVecView<'_> {
        RunVecView::Owned(&self.arena)
    }

    /// Arena slots in use, terminators included.
    pub fn len(&self) -> usize {
        self.arena.len()
    }

    /// Iterate the values of the run starting at `start`.
    pub fn run_at(&self, start: RunIndex) -> RunValues<'_> {
        self.view().run_at(start)
    }

    /// Whether one more slot fits under [`ARENA_CAP`].
    fn push_ok(&self) -> bool {
        self.arena.len() < ARENA_CAP
    }

    /// Start a new run holding `value` and return its start offset, or `None` when the
    /// arena is full.
    pub fn try_new_run(&mut self, value: u16) -> Option<RunIndex> {
        debug_assert_ne!(value, TERMINATOR, "the terminator is not a storable value");
        if self.arena.len() + 2 > ARENA_CAP {
            return None;
        }
        let start = self.arena.len() as u16;
        self.arena.push(value);
        self.arena.push(TERMINATOR);
        Some(RunIndex(start))
    }

    /// Panicking [`try_new_run`](Self::try_new_run), for the live-mutation path (mutable
    /// documents go wide in ADR 0002 PR 6; until then the ceiling is a hard error).
    pub fn new_run(&mut self, value: u16) -> RunIndex {
        self.try_new_run(value).expect(OVERFLOW_MSG)
    }

    /// Parse path: append `value` to the *trailing* run (the most recently created one,
    /// which `start` must address), returning `false` when the arena is full. A value
    /// already present in the run is deduplicated (`class="a b a"` keeps one `a`).
    #[must_use]
    pub fn try_append_last(&mut self, start: RunIndex, value: u16) -> bool {
        debug_assert_ne!(value, TERMINATOR, "the terminator is not a storable value");
        debug_assert_eq!(self.arena.last(), Some(&TERMINATOR));
        let term = self.arena.len() - 1;
        debug_assert!(
            !self.arena[start.as_usize()..term].contains(&TERMINATOR),
            "try_append_last must address the trailing run"
        );
        if self.arena[start.as_usize()..term].contains(&value) {
            return true;
        }
        if !self.push_ok() {
            return false;
        }
        self.arena[term] = value;
        self.arena.push(TERMINATOR);
        true
    }

    /// Live mutation: append `value` to the run at `start`, returning the run's (possibly
    /// new) start. The trailing run extends in place; any other run is relocated to the
    /// arena end — the caller must re-point its node slot when the returned start differs.
    /// A value already in the run is a no-op. Panics when the arena is full.
    pub fn append(&mut self, start: RunIndex, value: u16) -> RunIndex {
        debug_assert_ne!(value, TERMINATOR, "the terminator is not a storable value");
        let s = start.as_usize();
        let mut end = s;
        while self.arena[end] != TERMINATOR {
            if self.arena[end] == value {
                return start;
            }
            end += 1;
        }
        if end + 1 == self.arena.len() {
            assert!(self.push_ok(), "{OVERFLOW_MSG}");
            self.arena[end] = value;
            self.arena.push(TERMINATOR);
            return start;
        }
        let run_len = end - s;
        assert!(
            self.arena.len() + run_len + 2 <= ARENA_CAP,
            "{OVERFLOW_MSG}"
        );
        let new_start = self.arena.len() as u16;
        for i in s..end {
            let v = self.arena[i];
            self.arena.push(v);
        }
        self.arena.push(value);
        self.arena.push(TERMINATOR);
        RunIndex(new_start)
    }

    /// Live mutation: remove every value in `doomed` from the run at `start`, in place —
    /// survivors shift left and the run re-terminates early, stranding the freed slots as
    /// garbage until repackage. Returns the number removed and whether the run is now
    /// empty; an emptied run has no representation, so the caller must drop the node's
    /// pointer to it.
    pub fn remove(&mut self, start: RunIndex, doomed: &[u16]) -> (usize, bool) {
        let s = start.as_usize();
        let mut read = s;
        let mut write = s;
        while self.arena[read] != TERMINATOR {
            let value = self.arena[read];
            if !doomed.contains(&value) {
                self.arena[write] = value;
                write += 1;
            }
            read += 1;
        }
        let removed = read - write;
        if removed > 0 {
            self.arena[write] = TERMINATOR;
        }
        (removed, write == s)
    }
}

/// Borrowed, read-only view over a [`RunVec`] arena — owned native `u16`s or archived
/// little-endian ones read via `.to_native()`, so one read path serves both.
#[derive(Clone, Copy)]
pub(crate) enum RunVecView<'a> {
    Owned(&'a [u16]),
    Archived(&'a [rkyv::Archived<u16>]),
}

impl<'a> RunVecView<'a> {
    fn at(&self, index: usize) -> u16 {
        match self {
            Self::Owned(s) => s[index],
            Self::Archived(s) => s[index].to_native(),
        }
    }

    /// Iterate the values of the run starting at `start`.
    pub(crate) fn run_at(&self, start: RunIndex) -> RunValues<'a> {
        RunValues {
            view: *self,
            pos: start.as_usize(),
        }
    }

    /// Advance an externally-held cursor (an arena offset) one value — for lending
    /// iterators that cannot hold a borrow of the view across calls.
    pub(crate) fn next_in_run(&self, offset: &mut Option<u16>) -> Option<u16> {
        let pos = (*offset)? as usize;
        let value = self.at(pos);
        if value == TERMINATOR {
            *offset = None;
            return None;
        }
        *offset = Some(pos as u16 + 1);
        Some(value)
    }
}

/// Iterates a run's values up to its terminator.
pub struct RunValues<'a> {
    view: RunVecView<'a>,
    pos: usize,
}

impl Iterator for RunValues<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.view.at(self.pos);
        (value != TERMINATOR).then(|| {
            self.pos += 1;
            value
        })
    }
}

impl ArchivedRunVec {
    pub(crate) fn view(&self) -> RunVecView<'_> {
        RunVecView::Archived(self.arena.as_slice())
    }
}
