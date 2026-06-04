use crate::stores::{ListIndex, ListRemovalResult};

use super::{DataAttribute, DataAttributeStore};

/// These list functions are used by different implementation (one with direct access to a store ref another that uses a lock)
pub(crate) mod data_attr_list {
    use crate::stores::{DataAttribute, DataAttributeStore, ListIndex, ListRemovalResult};

    pub(crate) fn next<'a>(
        store: &'a DataAttributeStore,
        list_index: &mut Option<ListIndex>,
    ) -> Option<DataAttribute<'a>> {
        let index = (*list_index)?;
        let (next, val) = store.lists.next(index);
        let val = store.attribute_at(val.into());
        *list_index = next;
        Some(val)
    }

    pub(crate) fn remove<F: Fn(DataAttribute) -> bool>(
        store: &mut DataAttributeStore,
        index: ListIndex,
        f: F,
    ) -> usize {
        let to_remove = store
            .indexes(index)
            .filter(|i| f(store.attribute_at(*i)))
            .collect::<Vec<_>>();

        let mut count = 0;
        for i in to_remove {
            let res = store.lists.list_mut_at(index).remove(i.as_u16());
            if res != ListRemovalResult::NotFound {
                count += 1;
            }
        }

        count
    }

    pub(crate) fn delete(
        store: &mut DataAttributeStore,
        index: ListIndex,
        attr: &DataAttribute<'_>,
    ) -> ListRemovalResult {
        let Some(i) = store.binary_search(attr).ok() else {
            return ListRemovalResult::NotFound;
        };
        store.lists.list_mut_at(index).remove(i)
    }

    pub fn insert(store: &mut DataAttributeStore, index: ListIndex, attr: &DataAttribute<'_>) {
        let attr_index = store.get_or_insert(attr);
        store.lists.list_mut_at(index).append(attr_index);
    }
}

pub struct DataAttributeList<'a> {
    pub(super) store: &'a DataAttributeStore,
    pub(super) index: Option<ListIndex>,
}

impl<'a> Iterator for DataAttributeList<'a> {
    type Item = DataAttribute<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        data_attr_list::next(self.store, &mut self.index)
    }
}

pub struct DataAttributeListMut<'a> {
    pub(super) store: &'a mut DataAttributeStore,
    pub(super) index: ListIndex,
}

impl DataAttributeListMut<'_> {
    pub fn remove<F: Fn(DataAttribute) -> bool>(&mut self, f: F) -> usize {
        data_attr_list::remove(self.store, self.index, f)
    }

    #[must_use]
    /// when the list is removed, the remaining list head remains in place,
    /// which it is iportant to check the returned result and make sure that
    /// you don't use it again
    pub fn delete(&mut self, attr: &DataAttribute<'_>) -> ListRemovalResult {
        data_attr_list::delete(self.store, self.index, attr)
    }

    pub fn insert(&mut self, attr: &DataAttribute<'_>) {
        data_attr_list::insert(self.store, self.index, attr)
    }
}
