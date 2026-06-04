use crate::stores::{
    ListIndex,
    listvec::{ListRebuilder, ListRebuilt},
    stringheap::StringHeap,
};

use super::AttributeStore;

pub(crate) struct AttributeReBuilder {
    lists_rebuilder: ListRebuilder,
}

impl AttributeReBuilder {
    pub fn new(store: &AttributeStore) -> Self {
        Self {
            lists_rebuilder: ListRebuilder::new(store.lists.len(), store.attributes.len()),
        }
    }

    pub fn mark_list_used(&mut self, store: &AttributeStore, index: ListIndex) {
        self.lists_rebuilder.mark_list_used(&store.lists, index)
    }

    pub fn build(self, store: &AttributeStore) -> (Vec<Option<u16>>, AttributeStore) {
        let ListRebuilt {
            lists_reidx,
            value_reidx,
            lists,
        } = self.lists_rebuilder.build(&store.lists);

        let mut strings = StringHeap::with_capacity_as(&store.strings);
        let mut attributes: Vec<(u8, u16)> = Vec::with_capacity(store.attributes.len());

        let used_indexes = value_reidx
            .iter()
            .enumerate()
            .filter_map(|(n, i)| i.is_some().then_some(n));

        for old in used_indexes {
            let (tag, s_idx) = store.attributes[old];
            let s = &store.strings[s_idx];
            let new_s_idx = strings.insert(s);
            attributes.push((tag, new_s_idx));
        }

        let store = AttributeStore {
            lists,
            attributes,
            strings,
        };

        (lists_reidx, store)
    }
}
