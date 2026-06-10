use std::collections::BTreeMap;

use crate::stores::{ListIndex, listvec::ListVec, stringheap::StringHeap};

use super::ClassStore;

/// Builds the sorted class store during parsing. Class names are owned on insert (the
/// tokenizer hands back transient buffers, not input borrows); the map deduplicates
/// identical names before interning at [`build`](Self::build), so the output is
/// byte-identical to a borrowed-key build. `Box<str>: Borrow<str>` lets lookups borrow the
/// name as `&str`, so only the first occurrence of a name allocates.
#[derive(Default)]
pub struct ClassStoreBuilder {
    lists: ListVec,
    classes: BTreeMap<Box<str>, u16>,
    counter: u16,
    stringbytes: usize,
}

impl ClassStoreBuilder {
    pub fn add_class_list(&mut self, classes: &str) -> ListIndex {
        let mut classes = classes.split_ascii_whitespace();
        let first = classes.next().unwrap_or("");

        let index = self.add_list(first);
        for class in classes {
            let i = self.get_or_insert(class);
            self.lists.list_mut_at(index).append(i);
        }
        index
    }

    fn add_list(&mut self, class: &str) -> ListIndex {
        let i = self.get_or_insert(class);
        self.lists.new_list(i)
    }

    fn get_or_insert(&mut self, class: &str) -> u16 {
        if let Some(&i) = self.classes.get(class) {
            i
        } else {
            let i = self.counter;
            self.stringbytes += class.len();
            self.classes.insert(Box::from(class), i);
            self.counter += 1;
            i
        }
    }

    pub fn build(self) -> ClassStore {
        let ClassStoreBuilder {
            mut lists,
            classes,
            counter,
            stringbytes,
        } = self;

        let mut reidx = vec![u16::MAX; counter as usize];
        let mut strings = StringHeap::with_capacity(stringbytes, counter as usize);
        let mut attribs: Vec<u16> = Vec::with_capacity(counter as usize);

        for (new_index, (class, old_index)) in classes.into_iter().enumerate() {
            reidx[old_index as usize] = new_index as u16;
            let stringidx = strings.insert(&class);
            attribs.push(stringidx);
        }

        lists.reindex_value(&reidx);
        ClassStore {
            lists,
            classes: attribs,
            strings,
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
