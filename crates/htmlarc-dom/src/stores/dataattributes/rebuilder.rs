use crate::stores::{
    ListIndex,
    listvec::{ListRebuilder, ListRebuilt},
    stringheap::StringHeap,
};

use super::DataAttributeStore;

pub(crate) struct DataAttributeRebuilder {
    lists_rebuilder: ListRebuilder,
}

impl DataAttributeRebuilder {
    pub fn new(store: &DataAttributeStore) -> Self {
        Self {
            lists_rebuilder: ListRebuilder::new(store.lists.len(), store.attributes.len()),
        }
    }

    pub fn mark_list_used(&mut self, store: &DataAttributeStore, index: ListIndex) {
        self.lists_rebuilder.mark_list_used(&store.lists, index)
    }

    pub fn build(self, store: &DataAttributeStore) -> (Vec<Option<u16>>, DataAttributeStore) {
        let ListRebuilt {
            lists_reidx,
            value_reidx,
            lists,
        } = self.lists_rebuilder.build(&store.lists);

        let mut strings = StringHeap::with_capacity_as(&store.strings);
        let mut attributes = Vec::with_capacity(store.attributes.len());

        let used_indexes = value_reidx
            .iter()
            .enumerate()
            .filter_map(|(n, i)| i.is_some().then_some(n));

        for old in used_indexes {
            let (t_idx, s_idx) = store.attributes[old];
            let s = &store.strings[s_idx];
            let t = &store.strings[t_idx];
            let new_s_idx = strings.insert(s);
            let new_t_idx = strings.insert(t);
            attributes.push((new_t_idx, new_s_idx));
        }

        let store = DataAttributeStore {
            lists,
            attributes,
            strings,
        };

        (lists_reidx, store)
    }
}
