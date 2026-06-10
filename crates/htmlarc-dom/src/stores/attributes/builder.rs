use std::collections::BTreeMap;

use crate::entities;
use crate::stores::{ListIndex, listvec::ListVec, stringheap::StringHeap};

use super::{Attribute, AttributeStore};

#[derive(Default)]
pub struct AttributeStoreBuilder<'a> {
    lists: ListVec,
    /// Sorted vec
    attributes: BTreeMap<Attribute<'a>, u16>,
    counter: u16,
    stringbytes: usize,
}

impl<'a> AttributeStoreBuilder<'a> {
    pub fn new_list(&mut self, attr: Attribute<'a>) -> ListIndex {
        let i = self.get_or_insert(attr);
        self.lists.new_list(i)
    }

    pub fn add_attribute(&mut self, list_index: ListIndex, attr: Attribute<'a>) {
        let i = self.get_or_insert(attr);
        self.lists.list_mut_at(list_index).append(i);
    }

    fn get_or_insert(&mut self, attr: Attribute<'a>) -> u16 {
        if let Some(i) = self.attributes.get(&attr) {
            *i
        } else {
            let i = self.counter;
            self.stringbytes += attr.val.len();
            self.attributes.insert(attr, i);
            self.counter += 1;
            i
        }
    }

    pub fn build(self) -> AttributeStore {
        let AttributeStoreBuilder {
            mut lists,
            attributes,
            counter,
            stringbytes,
        } = self;

        let mut reidx = vec![u16::MAX; counter as usize];
        let mut strings = StringHeap::with_capacity(stringbytes, counter as usize);
        let mut attribs: Vec<(u8, u16)> = Vec::with_capacity(counter as usize);

        for (new_index, (attr, old_index)) in attributes.into_iter().enumerate() {
            reidx[old_index as usize] = new_index as u16;
            // Same fast path as text: skip the decoder unless the value holds a '&'.
            let stringidx = if attr.val.contains('&') {
                strings.insert(&entities::decode(attr.val))
            } else {
                strings.insert(attr.val)
            };
            attribs.push((attr.tag as u8, stringidx));
        }

        lists.reindex_value(&reidx);
        AttributeStore {
            lists,
            attributes: attribs,
            strings,
        }
    }
}
