use std::fmt::Display;

use rkyv::{Archive, Deserialize, Serialize};

use crate::stores::{ListIndex, listvec::ListVec, stringheap::StringHeap};

use super::{
    Class,
    lists::{ClassList, ClassListMut},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassIndex(u16);

impl ClassIndex {
    pub fn as_u16(&self) -> u16 {
        self.0
    }
}
impl Display for ClassIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for ClassIndex {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// Conceptually, works like a vector of lists. Internally
/// it's implemented as a vector of linked lists.
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct ClassStore {
    pub(super) lists: ListVec,
    /// sorted
    pub(super) classes: Vec<u16>,
    pub(super) strings: StringHeap,
}

impl ClassStore {
    pub fn with_capacity_as(other: &Self) -> Self {
        Self {
            lists: ListVec::with_capacity_as(&other.lists),
            classes: Vec::with_capacity(other.classes.len()),
            strings: StringHeap::with_capacity_as(&other.strings),
        }
    }
    pub fn add_list(&mut self, class: &Class) -> ListIndex {
        let class_index = self.get_or_insert(class);
        self.lists.new_list(class_index)
    }

    pub fn add_class_list(&mut self, classes: &str) -> Option<ListIndex> {
        let mut classes = classes.split_ascii_whitespace().map(Class);
        let first = classes.next().unwrap_or(Class(""));

        let index = self.add_list(&first);
        let mut list = self.list_mut_at(index);
        for class in classes {
            list.insert(&class);
        }
        Some(index)
    }

    pub fn list_at(&self, index: ListIndex) -> ClassList<'_> {
        ClassList {
            index: self.lists.head_index_at(index),
            store: self,
        }
    }

    pub fn list_mut_at(&mut self, index: ListIndex) -> ClassListMut<'_> {
        ClassListMut { store: self, index }
    }

    pub(crate) fn class_at(&self, index: ClassIndex) -> Class<'_> {
        let s = self.classes[index.0 as usize];
        Class(&self.strings[s])
    }

    pub(crate) fn get_or_insert(&mut self, class: &Class) -> u16 {
        match self.binary_search(class) {
            Ok(index) => index,
            Err(index) => {
                let val = self.strings.insert(class.0);
                if self.classes.len() == index as usize {
                    self.classes.push(val);
                } else {
                    self.classes.insert(index as usize, val);
                    self.lists.shift_values_from(index);
                }
                index
            }
        }
    }

    pub fn find(&self, class: &Class) -> Option<ClassIndex> {
        self.binary_search(class).ok().map(ClassIndex)
    }

    pub(super) fn binary_search(&self, searched: &Class) -> Result<u16, u16> {
        use std::cmp::Ordering::*;

        let mut size = self.classes.len();
        let mut left = 0;
        let mut right = size;
        while left < right {
            let mid = left + size / 2;

            let class = self.class_at(ClassIndex(mid as u16));
            let cmp = class.cmp(searched);

            left = if cmp == Less { mid + 1 } else { left };
            right = if cmp == Greater { mid } else { right };
            if cmp == Equal {
                return Ok(mid as u16);
            }

            size = right - left;
        }

        Err(left as u16)
    }

    /// Returns true if the class is present in the list.
    ///
    /// # Arguments
    /// - `class`: the class to check for
    pub fn has<P>(&self, class: &P) -> bool
    where
        P: for<'a> PartialEq<Class<'a>>,
    {
        self.classes.iter().any(|c| {
            let class_name = Class(&self.strings[*c]);

            class == &class_name
        })
    }

    pub fn indexes(
        &self,
        index: ListIndex,
    ) -> std::iter::Map<crate::stores::listvec::List<'_>, fn(u16) -> ClassIndex> {
        self.lists.list_at(index).map(ClassIndex)
    }
}
