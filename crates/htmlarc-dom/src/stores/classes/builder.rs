use crate::stores::interner::StringInterner;
use crate::stores::{ListIndex, listvec::ListVec};

use super::ClassStore;

/// Builds the class store during parsing. Each distinct class name is copied **once**,
/// straight into the interned [`StringHeap`] (no `Box<str>` intermediary); the
/// [`StringInterner`]'s hash table deduplicates names so a repeated class allocates
/// nothing. [`build`](Self::build) sorts the index table into name order — what the
/// runtime store's binary search expects, and identical to the old `BTreeMap` order.
#[derive(Default)]
pub struct ClassStoreBuilder {
    lists: ListVec,
    classes: StringInterner,
    /// Set (first reason wins) once a per-document u16 ceiling is hit; the parse path
    /// reads it via [`overflow`](Self::overflow) and discards the whole document.
    overflow: Option<&'static str>,
}

impl ClassStoreBuilder {
    /// The reason this builder overflowed a per-document capacity, if any.
    pub fn overflow(&self) -> Option<&'static str> {
        self.overflow
    }

    fn intern_or_poison(&mut self, s: &str) -> u16 {
        match self.classes.try_intern(s) {
            Some(i) => i,
            None => {
                self.overflow.get_or_insert("class strings exceed 65,535");
                0
            }
        }
    }

    pub fn add_class_list(&mut self, classes: &str) -> ListIndex {
        let mut names = classes.split_ascii_whitespace();
        let first = names.next().unwrap_or("");

        let first_i = self.intern_or_poison(first);
        let index = match self.lists.try_new_list(first_i) {
            Some(list) => list,
            None => {
                self.overflow
                    .get_or_insert("class list count exceeds 65,534");
                return ListIndex::from(0);
            }
        };
        for class in names {
            let i = self.intern_or_poison(class);
            if !self.lists.list_mut_at(index).try_append(i) {
                self.overflow
                    .get_or_insert("class list entries exceed 32,768");
                break;
            }
        }
        index
    }

    pub fn build(self) -> ClassStore {
        let ClassStoreBuilder {
            mut lists, classes, ..
        } = self;

        // Sort the interned indices into name order (the runtime store binary-searches the
        // class table). The heap stays in insertion order; the table points back into it by
        // original index, so each name is copied only once (at parse).
        let mut order: Vec<u16> = (0..classes.len()).collect();
        order.sort_unstable_by(|&a, &b| classes.get(a).cmp(classes.get(b)));

        let mut reidx = vec![0u16; order.len()];
        let mut table: Vec<u16> = Vec::with_capacity(order.len());
        for (new_index, &old) in order.iter().enumerate() {
            reidx[old as usize] = new_index as u16;
            table.push(old);
        }

        lists.reindex_value(&reidx);
        ClassStore {
            lists,
            classes: table,
            strings: classes.into_heap(),
        }
    }
}

#[test]
fn test_classes() {
    use insta::assert_snapshot;

    let mut bldr = ClassStoreBuilder::default();

    let l1 = bldr.add_class_list("one a two");
    let l2 = bldr.add_class_list("a one b");

    let lists = bldr.build();

    assert_snapshot!( lists.dbg(l1), @"one, a, two");
    assert_snapshot!( lists.dbg(l2), @"a, one, b");
}
