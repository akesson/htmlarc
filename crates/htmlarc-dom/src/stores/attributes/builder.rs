use std::collections::BTreeMap;

use crate::html::HtmlAttr;
use crate::stores::{ListIndex, listvec::ListVec, stringheap::StringHeap};

use super::AttributeStore;

/// Builds the sorted attribute store during parsing.
///
/// Attribute values arrive already entity-decoded (the tokenizer resolves character
/// references), so they are interned verbatim at [`build`](Self::build). Values are owned
/// on insert — exactly like data attributes — because the tokenizer hands back transient
/// buffers, not borrows of the input. The `(tag, value)` keys deduplicate identical
/// attributes before interning, so the output is byte-identical to a borrowed-key build.
///
/// The map is nested (`tag -> value -> index`) rather than keyed on a `(tag, value)` tuple
/// so that lookups can borrow the value as `&str` (`Box<str>: Borrow<str>`) and only the
/// first occurrence of a value allocates.
#[derive(Default)]
pub struct AttributeStoreBuilder {
    lists: ListVec,
    attributes: BTreeMap<HtmlAttr, BTreeMap<Box<str>, u16>>,
    counter: u16,
    stringbytes: usize,
}

impl AttributeStoreBuilder {
    pub fn new_list(&mut self, tag: HtmlAttr, val: &str) -> ListIndex {
        let i = self.get_or_insert(tag, val);
        self.lists.new_list(i)
    }

    pub fn add_attribute(&mut self, list_index: ListIndex, tag: HtmlAttr, val: &str) {
        let i = self.get_or_insert(tag, val);
        self.lists.list_mut_at(list_index).append(i);
    }

    fn get_or_insert(&mut self, tag: HtmlAttr, val: &str) -> u16 {
        if let Some(inner) = self.attributes.get(&tag)
            && let Some(&i) = inner.get(val)
        {
            return i;
        }
        let i = self.counter;
        self.stringbytes += val.len();
        self.attributes
            .entry(tag)
            .or_default()
            .insert(Box::from(val), i);
        self.counter += 1;
        i
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

        // Iterating `tag` then `value` yields the same total `(tag, value)` order a flat
        // sorted map would, so the assigned indices (and thus the output) are stable.
        let mut new_index = 0u16;
        for (tag, values) in attributes {
            for (val, old_index) in values {
                reidx[old_index as usize] = new_index;
                let stringidx = strings.insert(&val);
                attribs.push((tag as u8, stringidx));
                new_index += 1;
            }
        }

        lists.reindex_value(&reidx);
        AttributeStore {
            lists,
            attributes: attribs,
            strings,
        }
    }
}
