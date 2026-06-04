use std::fmt::{Debug, Display};

use insta::{assert_debug_snapshot, assert_snapshot};

use crate::stores::{ListIndex, ListRemovalResult};

use super::{
    List, ListVec,
    listentry::{ListEntry, ListInfo},
};

impl Display for ListVec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for index in self.list_iter() {
            writeln!(f, "{}", self.list_at(index).debug_string())?;
        }
        Ok(())
    }
}

impl List<'_> {
    fn debug_string(self) -> String {
        self.filter(|i| *i != 0)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Debug for ListVec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.vec
                .iter()
                .map(|entry| format!("{entry:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Debug for ListEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.value, self.info)
    }
}

impl Debug for ListInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let is_head = self.is_head();
        let next = self.next();
        match (is_head, next) {
            (true, Some(i)) => write!(f, "|->{i}"),
            (true, None) => write!(f, "|->|"),
            (false, Some(i)) => write!(f, "->{i}"),
            (false, None) => write!(f, "->|"),
        }
    }
}

#[test]
fn listvec_single() {
    let mut lists = ListVec::default();
    let l1 = lists.new_list(11);

    assert_eq!(1, lists.list_iter().count());

    let l1idx = lists.list_iter().next().unwrap();
    assert_snapshot!(lists.list_at(l1idx).debug_string(), @"11");

    assert_snapshot!(l1, @"0");

    lists.list_mut_at(l1).append(22);
    assert_debug_snapshot!(lists, @"11 |->1, 22 ->|");

    lists.list_mut_at(l1).append(33);
    assert_debug_snapshot!(lists, @"11 |->1, 22 ->2, 33 ->|");

    let l1_content = lists.list_at(l1).debug_string();
    assert_snapshot!(l1_content, @"11, 22, 33");

    assert_snapshot!(lists, @r###"
    11, 22, 33
    "###);

    let res = lists.list_mut_at(l1).remove(33);

    assert_eq!(res, ListRemovalResult::EntryRemoved);

    assert_snapshot!(lists, @r###"
    11, 22
    "###);
}

#[test]
fn listvec_remove_front() {
    let (l1, mut lists) = list(&[11, 22, 33]);

    lists.list_mut_at(l1).remove(11);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"22, 33");
    lists.list_mut_at(l1).remove(22);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"33");
    lists.list_mut_at(l1).remove(33);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"");
}

#[test]
fn listvec_remove_back() {
    let (l1, mut lists) = list(&[11, 22, 33]);

    lists.list_mut_at(l1).remove(33);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"11, 22");
    lists.list_mut_at(l1).remove(22);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"11");
    lists.list_mut_at(l1).remove(11);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"");
}

#[test]
fn listvec_remove_mid() {
    let (l1, mut lists) = list(&[11, 22, 33]);

    lists.list_mut_at(l1).remove(22);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"11, 33");
}
#[test]
fn listvec_remove_and_readd() {
    let (l1, mut lists) = list(&[11, 22]);

    lists.list_mut_at(l1).remove(22);
    lists.list_mut_at(l1).remove(11);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"");
    lists.list_mut_at(l1).append(33);
    assert_snapshot!(lists.list_at(l1).debug_string(), @"33");
}

fn list(vals: &[u16]) -> (ListIndex, ListVec) {
    let mut lists = ListVec::default();
    let mut iter = vals.iter();

    let first = iter.next().unwrap();
    let l1 = lists.new_list(*first);

    for entry in iter {
        lists.list_mut_at(l1).append(*entry);
    }

    (l1, lists)
}
