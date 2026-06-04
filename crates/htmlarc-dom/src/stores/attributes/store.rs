use std::fmt::Display;

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    html::HtmlAttr,
    stores::{
        listvec::{ListIndex, ListVec},
        stringheap::StringHeap,
    },
};

use super::{
    Attribute,
    lists::{AttributeList, AttributeListMut},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttrIndex(u16);

impl AttrIndex {
    pub fn as_u16(&self) -> u16 {
        self.0
    }
    pub fn as_usize(&self) -> usize {
        self.0 as usize
    }
}
impl Display for AttrIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for AttrIndex {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// Conceptually, works like a vector of lists. Internally
/// it's implemented as a vector of linked lists.
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct AttributeStore {
    pub(crate) lists: ListVec,
    /// Sorted vec
    pub(crate) attributes: Vec<(u8, u16)>,
    pub(crate) strings: StringHeap,
}

impl AttributeStore {
    pub fn with_capacity_as(other: &Self) -> Self {
        Self {
            lists: ListVec::with_capacity_as(&other.lists),
            attributes: Vec::with_capacity(other.attributes.len()),
            strings: StringHeap::with_capacity_as(&other.strings),
        }
    }

    pub fn add_list(&mut self, attr: &Attribute) -> ListIndex {
        let attr_index = self.get_or_insert(attr);

        self.lists.new_list(attr_index)
    }

    pub fn list_at(&self, index: ListIndex) -> AttributeList<'_> {
        AttributeList {
            store: self,
            index: self.lists.head_index_at(index),
        }
    }

    pub fn list_mut_at(&mut self, index: ListIndex) -> AttributeListMut<'_> {
        AttributeListMut { store: self, index }
    }

    pub fn attribute_at(&self, index: AttrIndex) -> Attribute<'_> {
        let (attr, s) = self.attributes[index.0 as usize];
        Attribute {
            tag: HtmlAttr::from_repr(attr).unwrap(),
            val: &self.strings[s],
        }
    }

    pub(super) fn get_or_insert(&mut self, attr: &Attribute) -> u16 {
        match self.binary_search(attr) {
            Ok(index) => index,
            Err(index) => {
                let val = self.strings.insert(attr.val);
                if self.attributes.len() == index as usize {
                    self.attributes.push((attr.tag as u8, val));
                } else {
                    self.attributes
                        .insert(index as usize, (attr.tag as u8, val));
                    self.lists.shift_values_from(index);
                }
                index
            }
        }
    }

    pub fn find(&self, attr: &Attribute) -> Option<AttrIndex> {
        self.binary_search(attr).ok().map(AttrIndex)
    }

    pub(super) fn binary_search(&self, searched: &Attribute) -> Result<u16, u16> {
        use std::cmp::Ordering::*;

        let mut size = self.attributes.len();
        let mut left = 0;
        let mut right = size;
        while left < right {
            let mid = left + size / 2;

            let cmp = searched.cmp(&self.attribute_at(AttrIndex(mid as u16)));

            left = if cmp == Less { mid + 1 } else { left };
            right = if cmp == Greater { mid } else { right };
            if cmp == Equal {
                return Ok(mid as u16);
            }

            size = right - left;
        }

        Err(left as u16)
    }

    pub fn indexes(&self, index: ListIndex) -> impl Iterator<Item = AttrIndex> + '_ {
        self.lists.list_at(index).map(AttrIndex)
    }

    #[cfg(test)]
    pub fn dbg_all(&self) -> String {
        self.lists
            .list_iter()
            .map(|i| {
                format!(
                    "{i}: {}",
                    self.list_at(i)
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
