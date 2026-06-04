use std::fmt::Display;

use rkyv::{Archive, Deserialize, Serialize};

use crate::stores::{
    listvec::{ListIndex, ListVec},
    stringheap::StringHeap,
};

use super::{
    DataAttribute,
    lists::{DataAttributeList, DataAttributeListMut},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataAttrIndex(u16);

impl DataAttrIndex {
    pub fn as_u16(&self) -> u16 {
        self.0
    }
}
impl Display for DataAttrIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for DataAttrIndex {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// Conceptually, works like a vector of lists. Internally
/// it's implemented as a vector of linked lists.
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct DataAttributeStore {
    pub(crate) lists: ListVec,
    /// Sorted vec
    pub(crate) attributes: Vec<(u16, u16)>,
    pub(crate) strings: StringHeap,
}

impl DataAttributeStore {
    pub fn with_capacity_as(other: &Self) -> Self {
        Self {
            lists: ListVec::with_capacity_as(&other.lists),
            attributes: Vec::with_capacity(other.attributes.len()),
            strings: StringHeap::with_capacity_as(&other.strings),
        }
    }

    pub fn add_list(&mut self, attr: &DataAttribute) -> ListIndex {
        let attr_index = self.get_or_insert(attr);

        self.lists.new_list(attr_index)
    }

    pub fn list_at(&self, index: ListIndex) -> DataAttributeList<'_> {
        DataAttributeList {
            store: self,
            index: self.lists.head_index_at(index),
        }
    }

    pub fn add_attribute(&mut self, list_index: ListIndex, attr: &DataAttribute) {
        let attr_index = self.get_or_insert(attr);
        self.lists.list_mut_at(list_index).append(attr_index);
    }

    pub fn list_mut_at(&mut self, index: ListIndex) -> DataAttributeListMut<'_> {
        DataAttributeListMut { store: self, index }
    }

    pub fn attribute_at(&self, index: DataAttrIndex) -> DataAttribute<'_> {
        let (tag, val) = self.attributes[index.0 as usize];
        DataAttribute {
            tag: &self.strings[tag],
            val: &self.strings[val],
        }
    }

    pub(super) fn get_or_insert(&mut self, attr: &DataAttribute) -> u16 {
        match self.binary_search(attr) {
            Ok(index) => index,
            Err(index) => {
                let val = self.strings.insert(attr.val);
                let tag = self.strings.insert(attr.tag);
                if self.attributes.len() == index as usize {
                    self.attributes.push((tag, val));
                } else {
                    self.attributes.insert(index as usize, (tag, val));
                    self.lists.shift_values_from(index);
                }
                index
            }
        }
    }

    pub fn find(&self, attr: &DataAttribute) -> Option<DataAttrIndex> {
        self.binary_search(attr).ok().map(DataAttrIndex)
    }

    pub(super) fn binary_search(&self, searched: &DataAttribute) -> Result<u16, u16> {
        use std::cmp::Ordering::*;

        let mut size = self.attributes.len();
        let mut left = 0;
        let mut right = size;
        while left < right {
            let mid = left + size / 2;

            let cmp = searched.cmp(&self.attribute_at(DataAttrIndex(mid as u16)));

            left = if cmp == Less { mid + 1 } else { left };
            right = if cmp == Greater { mid } else { right };
            if cmp == Equal {
                return Ok(mid as u16);
            }

            size = right - left;
        }

        Err(left as u16)
    }

    pub fn indexes(
        &self,
        index: ListIndex,
    ) -> std::iter::Map<crate::stores::listvec::List<'_>, fn(u16) -> DataAttrIndex> {
        self.lists.list_at(index).map(DataAttrIndex)
    }
}
