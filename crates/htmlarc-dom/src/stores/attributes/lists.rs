use crate::stores::{
    ListIndex, ListRemovalResult,
    attributes::{Attribute, store::AttributeStore},
};

/// These list functions are used by different implementation (one with direct access to a store ref another that uses a lock)
pub(crate) mod attr_list {
    use crate::stores::{
        ListIndex, ListRemovalResult,
        attributes::{Attribute, store::AttributeStore},
    };

    pub(crate) fn next<'a>(
        store: &'a AttributeStore,
        list_index: &mut Option<ListIndex>,
    ) -> Option<Attribute<'a>> {
        let index = (*list_index)?;
        let (next, val) = store.lists.next(index);
        let val = store.attribute_at(val.into());
        *list_index = next;
        Some(val)
    }

    pub(crate) fn remove<F: Fn(Attribute) -> bool>(
        store: &mut AttributeStore,
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
        store: &mut AttributeStore,
        index: ListIndex,
        attr: &Attribute<'_>,
    ) -> ListRemovalResult {
        let Some(i) = store.binary_search(attr).ok() else {
            return ListRemovalResult::NotFound;
        };
        store.lists.list_mut_at(index).remove(i)
    }

    pub fn insert(store: &mut AttributeStore, index: ListIndex, attr: &Attribute<'_>) {
        let attr_index = store.get_or_insert(attr);
        store.lists.list_mut_at(index).append(attr_index);
    }
}

pub struct AttributeList<'a> {
    pub(super) store: &'a AttributeStore,
    pub(super) index: Option<ListIndex>,
}

impl<'a> Iterator for AttributeList<'a> {
    type Item = Attribute<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        attr_list::next(self.store, &mut self.index)
    }
}

pub struct AttributeListMut<'a> {
    pub(super) store: &'a mut AttributeStore,
    pub(super) index: ListIndex,
}

impl AttributeListMut<'_> {
    pub fn remove<F: Fn(Attribute) -> bool>(&mut self, f: F) -> usize {
        attr_list::remove(self.store, self.index, f)
    }

    #[must_use]
    /// when the list is removed, the remaining list head remains in place,
    /// which it is iportant to check the returned result and make sure that
    /// you don't use it again
    pub fn delete(&mut self, attr: &Attribute<'_>) -> ListRemovalResult {
        attr_list::delete(self.store, self.index, attr)
    }

    pub fn insert(&mut self, attr: &Attribute<'_>) {
        attr_list::insert(self.store, self.index, attr)
    }
}
