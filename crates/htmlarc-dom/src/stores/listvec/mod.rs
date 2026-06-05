mod listentry;
mod rebuilder;

#[cfg(test)]
mod tests;

use std::fmt::Display;

use super::ListRemovalResult;
use listentry::{ArchivedListEntry, ListEntry};
pub(crate) use rebuilder::{ListRebuilder, ListRebuilt};
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListIndex(u16);

impl ListIndex {
    pub fn as_u16(&self) -> u16 {
        self.0
    }
    pub fn as_usize(&self) -> usize {
        self.0 as usize
    }
}
impl Display for ListIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for ListIndex {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// A vector of single-linked lists. The start of a list is marked with
/// a head list entry (first bit is set), a head is always created with
/// a value, but that value can be unset, meaning that a head can have
/// no value.
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct ListVec {
    pub(super) vec: Vec<ListEntry>,
}

impl ListVec {
    pub fn with_capacity_as(other: &Self) -> Self {
        let vec = Vec::with_capacity(other.vec.len());
        Self { vec }
    }

    pub(crate) fn view(&self) -> ListVecView<'_> {
        ListVecView::Owned(&self.vec)
    }

    pub(crate) fn head_index_at(&self, index: ListIndex) -> Option<ListIndex> {
        self.view().head_index_at(index)
    }

    pub fn next(&self, index: ListIndex) -> (Option<ListIndex>, u16) {
        self.view().next(index)
    }

    pub(super) fn shift_values_from(&mut self, start: u16) {
        for entry in &mut self.vec {
            if entry.value >= start {
                entry.value += 1;
            }
        }
    }
    pub fn new_list(&mut self, value: u16) -> ListIndex {
        let index = self.vec.len() as u16;
        self.vec.push(ListEntry::new_head(value));
        ListIndex(index)
    }

    pub fn list_at(&self, index: ListIndex) -> List<'_> {
        let index = (!self.vec[index.0 as usize].is_empty_head()).then_some(index);
        List { lists: self, index }
    }

    pub fn list_mut_at(&mut self, index: ListIndex) -> ListMut<'_> {
        ListMut {
            vec: &mut self.vec,
            index: index.0,
        }
    }

    pub fn list_iter(&self) -> impl Iterator<Item = ListIndex> + '_ {
        self.vec
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.info.is_head() && !entry.is_empty_head())
            .map(|(index, _)| (index as u16).into())
    }

    pub fn rebuild(&self, value_reidx: &[Option<u16>], list_reidx: &[Option<u16>]) -> Self {
        let mut new_list = Self::with_capacity_as(self);
        for (old, &new) in list_reidx.iter().enumerate() {
            if new.is_some() {
                let ListEntry { info, value } = self.vec[old];
                let new_val = value_reidx[value as usize].expect("all values should be reindexed");
                let new_info = info.reindexed(list_reidx);
                new_list.vec.push(ListEntry {
                    info: new_info,
                    value: new_val,
                });
            }
        }
        new_list
    }

    pub fn reindex_value(&mut self, reidx: &[u16]) {
        for entry in &mut self.vec {
            entry.value = reidx[entry.value as usize];
        }
    }

    pub fn len(&self) -> usize {
        self.vec.len()
    }
}

pub struct List<'a> {
    lists: &'a ListVec,
    index: Option<ListIndex>,
}

impl Iterator for List<'_> {
    type Item = u16;
    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index?;
        let (next, value) = self.lists.next(index);
        self.index = next;
        Some(value)
    }
}

/// A borrowed, read-only view over a [`ListVec`] — backed by either the owned
/// `[ListEntry]` slice or the archived one. Each entry is decoded into an owned
/// (Copy) `ListEntry` so the existing bit logic is reused verbatim.
#[derive(Clone, Copy)]
pub(crate) enum ListVecView<'a> {
    Owned(&'a [ListEntry]),
    Archived(&'a [ArchivedListEntry]),
}

impl<'a> ListVecView<'a> {
    fn entry(&self, index: usize) -> ListEntry {
        match self {
            Self::Owned(v) => v[index],
            Self::Archived(v) => v[index].decode(),
        }
    }

    pub(crate) fn head_index_at(&self, index: ListIndex) -> Option<ListIndex> {
        (!self.entry(index.0 as usize).is_empty_head()).then_some(index)
    }

    pub(crate) fn next(&self, index: ListIndex) -> (Option<ListIndex>, u16) {
        let entry = self.entry(index.0 as usize);
        let next = entry.info.next().map(|v| v.into());
        (next, entry.value)
    }
}

impl ArchivedListVec {
    pub(crate) fn view(&self) -> ListVecView<'_> {
        ListVecView::Archived(self.vec.as_slice())
    }
}

enum Insert {
    Head(usize),
    Tail(usize),
    AlreadyInserted,
}

pub struct ListMut<'a> {
    vec: &'a mut Vec<ListEntry>,
    index: u16,
}

impl ListMut<'_> {
    pub fn append(&mut self, value: u16) {
        match self.insert_index(value) {
            Insert::AlreadyInserted => {}
            Insert::Head(index) => self.vec[index].value = value,
            Insert::Tail(last) => {
                let index = self.vec.len();
                self.vec[last].info.set_next(index as u16);
                self.vec.push(ListEntry::tail(value));
            }
        }
    }

    fn insert_index(&self, value: u16) -> Insert {
        let mut index = self.index as usize;

        // the first index is the head of the list
        if self.vec[index].is_empty_head() {
            return Insert::Head(index);
        }

        loop {
            if self.vec[index].value == value {
                return Insert::AlreadyInserted;
            }
            if let Some(next) = self.vec[index].info.next() {
                index = next as usize;
            } else {
                break;
            }
        }
        Insert::Tail(index)
    }

    pub fn remove(&mut self, value: u16) -> ListRemovalResult {
        let Some(Position { previous, current }) = self.position(value) else {
            return ListRemovalResult::NotFound;
        };
        let new_next = self.vec[current].info.next();

        if let Some(previous) = previous {
            self.vec[previous].info.set_next_opt(new_next);
            self.vec[current].info.unset_next();
            ListRemovalResult::EntryRemoved
        } else if let Some(next) = new_next {
            let next_entry = self.vec[next as usize];
            self.vec[current].info.set_next_opt(next_entry.info.next());
            self.vec[current].value = next_entry.value;
            self.vec[next as usize].unset();
            ListRemovalResult::EntryRemoved
        } else {
            self.vec[current].unset();
            ListRemovalResult::ListRemoved
        }
    }

    fn position(&self, value: u16) -> Option<Position> {
        let mut previous = None;
        let mut current = self.index as usize;
        loop {
            if self.vec[current].value == value {
                return Some(Position { previous, current });
            }
            if let Some(next) = self.vec[current].info.next() {
                previous = Some(current);
                current = next as usize;
            } else {
                return None;
            }
        }
    }
}

struct Position {
    previous: Option<usize>,
    current: usize,
}
