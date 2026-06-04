use crate::stores::{ListIndex, ListRemovalResult};

use super::{Class, store::ClassStore};

pub(crate) mod class_list {
    use crate::stores::{Class, ClassStore, ListIndex, ListRemovalResult};

    pub(crate) fn next<'a>(
        store: &'a ClassStore,
        list_index: &mut Option<ListIndex>,
    ) -> Option<Class<'a>> {
        let index = (*list_index)?;
        let (next, val) = store.lists.next(index);
        let val = store.class_at(val.into());
        *list_index = next;
        Some(val)
    }

    pub(crate) fn remove<F: Fn(&str) -> bool>(
        store: &mut ClassStore,
        index: ListIndex,
        f: F,
    ) -> usize {
        let to_remove = store
            .indexes(index)
            .filter(|i| f(store.class_at(*i).0))
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
}

pub struct ClassList<'a> {
    pub(crate) store: &'a ClassStore,
    pub(crate) index: Option<ListIndex>,
}

impl<'a> Iterator for ClassList<'a> {
    type Item = Class<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        class_list::next(self.store, &mut self.index)
    }
}

impl ClassList<'_> {
    pub fn write_to(self, string: &mut String) {
        string.push_str(&format!(
            "class=\"{}\"",
            self.map(|class| class.0).collect::<Vec<_>>().join(" ")
        ));
    }
}

pub struct ClassListMut<'a> {
    pub(crate) store: &'a mut ClassStore,
    pub(crate) index: ListIndex,
}

impl ClassListMut<'_> {
    #[must_use]
    /// when the list is removed, the remaining list head remains in place,
    /// which it is iportant to check the returned result and make sure that
    /// you don't use it again
    pub fn delete(&mut self, class: &Class<'_>) -> ListRemovalResult {
        let Some(i) = self.store.binary_search(class).ok() else {
            return ListRemovalResult::NotFound;
        };
        self.store.lists.list_mut_at(self.index).remove(i)
    }

    pub fn insert(&mut self, class: &Class<'_>) {
        let index = self.store.get_or_insert(class);
        self.store.lists.list_mut_at(self.index).append(index);
    }
}
