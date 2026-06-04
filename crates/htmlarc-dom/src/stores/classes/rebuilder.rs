use crate::stores::{
    ListIndex,
    listvec::{ListRebuilder, ListRebuilt},
    stringheap::StringHeap,
};

use super::ClassStore;

pub(crate) struct ClassReBuilder {
    lists_rebuilder: ListRebuilder,
}

impl ClassReBuilder {
    pub fn new(store: &ClassStore) -> Self {
        Self {
            lists_rebuilder: ListRebuilder::new(store.lists.len(), store.classes.len()),
        }
    }

    pub fn mark_list_used(&mut self, store: &ClassStore, index: ListIndex) {
        self.lists_rebuilder.mark_list_used(&store.lists, index)
    }

    pub fn build(self, store: &ClassStore) -> (Vec<Option<u16>>, ClassStore) {
        let ListRebuilt {
            lists_reidx,
            value_reidx,
            lists,
        } = self.lists_rebuilder.build(&store.lists);

        let mut strings = StringHeap::with_capacity_as(&store.strings);
        let mut classes: Vec<u16> = Vec::with_capacity(store.classes.len());

        let used_indexes = value_reidx
            .iter()
            .enumerate()
            .filter_map(|(n, i)| i.is_some().then_some(n));

        for old in used_indexes {
            let s_idx = store.classes[old];
            let s = &store.strings[s_idx];
            let new_s_idx = strings.insert(s);
            classes.push(new_s_idx);
        }

        let store = ClassStore {
            lists,
            classes,
            strings,
        };

        (lists_reidx, store)
    }
}
