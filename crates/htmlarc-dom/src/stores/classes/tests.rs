use insta::assert_snapshot;

use crate::stores::{ListIndex, ListRemovalResult};

use super::{Class, ClassStore};

impl ClassStore {
    pub fn dbg(&self, index: ListIndex) -> String {
        self.list_at(index)
            .map(|a| a.0.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[test]
fn classes_add_and_remove() {
    let (index, mut lists) = list(&["val1", "val2", "val1"]);
    assert_snapshot!(lists.dbg(index), @"val1, val2");

    lists.list_mut_at(index).insert(&to_class(""));
    assert_snapshot!(lists.dbg(index), @r###"val1, val2, "###);

    // remove last
    let ret = lists.list_mut_at(index).delete(&to_class(""));
    assert_eq!(ret, ListRemovalResult::EntryRemoved);
    assert_snapshot!(lists.dbg(index), @"val1, val2");

    // doesn't exist
    let ret = lists.list_mut_at(index).delete(&to_class("val3"));
    assert_eq!(ret, ListRemovalResult::NotFound);
    assert_snapshot!(lists.dbg(index), @"val1, val2");

    // remove remaining
    let ret = lists.list_mut_at(index).delete(&to_class("val1"));
    assert_eq!(ret, ListRemovalResult::EntryRemoved);
    assert_snapshot!(lists.dbg(index), @"val2");

    let ret = lists.list_mut_at(index).delete(&to_class("val2"));
    assert_eq!(ret, ListRemovalResult::ListRemoved);
    assert_snapshot!(lists.dbg(index), @"");
}

fn list(classes: &[&str]) -> (ListIndex, ClassStore) {
    let mut lists = ClassStore::default();

    let mut iter = classes.iter().map(|class| Class(class));

    let class = iter.next().unwrap();

    let l1 = lists.add_list(&class);

    for class in iter {
        lists.list_mut_at(l1).insert(&class);
    }
    (l1, lists)
}

fn to_class(val: &str) -> Class<'_> {
    Class(val)
}
