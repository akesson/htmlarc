use insta::assert_snapshot;

use crate::{
    html::HtmlAttr::{self, label, style, title},
    stores::{AttributeReBuilder, ListIndex, ListRemovalResult},
};

use super::{Attribute, AttributeStore};
const A1: Attribute = to_attr(title, "title1");
const A2: Attribute = to_attr(label, "mylable");
const A3: Attribute = to_attr(style, "color");
const A4: Attribute = to_attr(title, "title2");

#[test]
fn attributes_add_and_remove() {
    let (index, mut lists) = list(&[A1, A2, A1]);
    assert_snapshot!(lists.dbg(index), @r###"title="title1", label="mylable""###);

    lists.list_mut_at(index).insert(&A3);
    assert_snapshot!(lists.dbg(index), @r###"title="title1", label="mylable", style="color""###);

    // remove last
    let ret = lists.list_mut_at(index).delete(&A3);
    assert_eq!(ret, ListRemovalResult::EntryRemoved);
    assert_snapshot!(lists.dbg(index), @r###"title="title1", label="mylable""###);

    // doesn't exist
    let ret = lists.list_mut_at(index).delete(&A4);
    assert_eq!(ret, ListRemovalResult::NotFound);
    assert_snapshot!(lists.dbg(index), @r###"title="title1", label="mylable""###);

    // remove remaining
    let ret = lists.list_mut_at(index).delete(&A1);
    assert_eq!(ret, ListRemovalResult::EntryRemoved);
    let ret = lists.list_mut_at(index).delete(&A2);
    assert_eq!(ret, ListRemovalResult::ListRemoved);

    assert_snapshot!(lists.dbg(index), @"");
}

#[test]
fn reindex_list() {
    let (l1, mut lists) = list(&[A1, A2]);
    let l2 = add_list(&mut lists, &[A3, A1]);
    let l3 = add_list(&mut lists, &[A4]);
    assert_snapshot!(lists.dbg_all(), @r###"
    0: title="title1", label="mylable"
    2: style="color", title="title1"
    4: title="title2"
    "###);

    let r = lists.list_mut_at(l3).delete(&A4);
    assert_eq!(r, ListRemovalResult::ListRemoved);
    let r = lists.list_mut_at(l1).delete(&A1);
    assert_eq!(r, ListRemovalResult::EntryRemoved);
    assert_snapshot!(lists.dbg_all(), @r###"
    0: label="mylable"
    2: style="color", title="title1"
    "###);

    let mut rebuilder = AttributeReBuilder::new(&lists);
    rebuilder.mark_list_used(&lists, l1);
    rebuilder.mark_list_used(&lists, l2);
    let (reindex, new) = rebuilder.build(&lists);

    assert_snapshot!(format!("{reindex:?}"), @"[Some(0), None, Some(1), Some(2), None]");
    assert_snapshot!(new.dbg_all(), @r###"
    0: label="mylable"
    1: style="color", title="title1"
    "###);
}

fn list(attrs: &[Attribute]) -> (ListIndex, AttributeStore) {
    let mut lists = AttributeStore::default();
    let idx = add_list(&mut lists, attrs);
    (idx, lists)
}

fn add_list(list: &mut AttributeStore, attrs: &[Attribute]) -> ListIndex {
    let mut iter = attrs.iter();

    let attrib = iter.next().unwrap();

    let idx = list.add_list(attrib);

    for attrib in iter {
        list.list_mut_at(idx).insert(attrib);
    }
    idx
}

const fn to_attr(tag: HtmlAttr, val: &str) -> Attribute<'_> {
    Attribute { tag, val }
}

impl AttributeStore {
    fn dbg(&self, index: ListIndex) -> String {
        self.list_at(index)
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
