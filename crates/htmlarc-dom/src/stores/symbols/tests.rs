use rkyv::rancor::Error;

use crate::stores::listvec::{ListRebuilder, ListVec};
use crate::stores::{ListIndex, ListRemovalResult};

use super::table::ArchivedSymbolTable;
use super::{LOCAL_CAP, Sym, SymbolTable, SymbolTableBuilder};

/// Intern a sequence through the parse builder, returning the finished table.
fn built(strings: &[&str]) -> SymbolTable {
    let mut b = SymbolTableBuilder::default();
    for s in strings {
        b.intern_or_poison(s);
    }
    b.build()
}

/// Compose a `SymbolTable` + `ListVec` the way the class accessors do, so the add/remove
/// behaviour ported from the former `ClassStore` tests is exercised end-to-end.
fn class_list(tokens: &[&str]) -> (SymbolTable, ListVec, ListIndex) {
    let mut symbols = SymbolTable::default();
    let mut lists = ListVec::default();
    let mut iter = tokens.iter();
    let first = symbols.get_or_insert(iter.next().unwrap());
    let head = lists.new_list(first.as_u16());
    for t in iter {
        let sym = symbols.get_or_insert(t);
        lists.list_mut_at(head).append(sym.as_u16());
    }
    (symbols, lists, head)
}

fn dbg(symbols: &SymbolTable, lists: &ListVec, head: ListIndex) -> String {
    lists
        .list_at(head)
        .map(|v| symbols.get(Sym(v)).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// Ported from the former `ClassStore::classes_add_and_remove`: insert (with dedup), remove
// by string, NotFound, and the EntryRemoved → ListRemoved transitions, now composed over a
// SymbolTable + ListVec directly.
#[test]
fn class_list_add_and_remove() {
    let (mut symbols, mut lists, head) = class_list(&["val1", "val2", "val1"]);
    assert_eq!(dbg(&symbols, &lists, head), "val1, val2");

    // insert the empty class
    let s = symbols.get_or_insert("");
    lists.list_mut_at(head).append(s.as_u16());
    assert_eq!(dbg(&symbols, &lists, head), "val1, val2, ");

    // remove the last (empty) entry
    let s = symbols.find("").unwrap();
    assert_eq!(
        lists.list_mut_at(head).remove(s.as_u16()),
        ListRemovalResult::EntryRemoved
    );
    assert_eq!(dbg(&symbols, &lists, head), "val1, val2");

    // a class that was never interned is simply absent
    assert_eq!(symbols.find("val3"), None);

    // remove down to one, then empty the list entirely
    let s = symbols.find("val1").unwrap();
    assert_eq!(
        lists.list_mut_at(head).remove(s.as_u16()),
        ListRemovalResult::EntryRemoved
    );
    assert_eq!(dbg(&symbols, &lists, head), "val2");

    let s = symbols.find("val2").unwrap();
    assert_eq!(
        lists.list_mut_at(head).remove(s.as_u16()),
        ListRemovalResult::ListRemoved
    );
    assert_eq!(dbg(&symbols, &lists, head), "");
}

#[test]
fn ids_are_insertion_ordered_find_is_content_sorted() {
    // Insertion order: "two"=0, "one"=1, ""=2, "ten"=3; "one" repeated allocates nothing.
    let t = built(&["two", "one", "", "ten", "one"]);
    assert_eq!(t.len(), 4);
    assert_eq!(t.get(Sym(0)), "two");
    assert_eq!(t.get(Sym(1)), "one");
    assert_eq!(t.get(Sym(2)), "");
    assert_eq!(t.get(Sym(3)), "ten");

    // find resolves the *stable* id regardless of sort position.
    assert_eq!(t.find(""), Some(Sym(2)));
    assert_eq!(t.find("one"), Some(Sym(1)));
    assert_eq!(t.find("ten"), Some(Sym(3)));
    assert_eq!(t.find("two"), Some(Sym(0)));
    assert_eq!(t.find("missing"), None);
}

#[test]
fn handles_non_ascii() {
    let t = built(&["café", "naïve", "café"]);
    assert_eq!(t.len(), 2);
    assert_eq!(t.find("café"), Some(Sym(0)));
    assert_eq!(t.find("naïve"), Some(Sym(1)));
}

#[test]
fn live_insert_keeps_permutation_sorted() {
    let mut t = SymbolTable::default();
    // Interleave inserts in deliberately unsorted order; ids stay insertion-ordered.
    for (i, s) in ["m", "a", "z", "f", "a", "c"].iter().enumerate() {
        let sym = t.get_or_insert(s);
        if *s == "a" && i > 0 {
            assert_eq!(sym, Sym(1), "repeated insert returns the original id");
        }
    }
    assert_eq!(t.len(), 5); // m,a,z,f,c (the second "a" allocated nothing)
    // Ids stay insertion-ordered…
    assert_eq!(t.get(Sym(0)), "m");
    assert_eq!(t.get(Sym(1)), "a");
    // …while every distinct string remains findable at its stable id.
    for (id, s) in ["m", "a", "z", "f", "c"].iter().enumerate() {
        assert_eq!(t.find(s), Some(Sym(id as u16)));
    }
    // The permutation that backs `find` is content-sorted after the interleaved inserts.
    assert_eq!(t.permutation_strings(), vec!["a", "c", "f", "m", "z"]);
}

#[test]
fn try_get_or_insert_caps_at_local_cap() {
    let mut t = SymbolTable::default();
    for i in 0..LOCAL_CAP as u32 {
        assert_eq!(t.try_get_or_insert(&i.to_string()), Some(Sym(i as u16)));
    }
    assert_eq!(t.len(), LOCAL_CAP);
    // A brand-new string past the cap is refused, leaving the heap untouched…
    assert_eq!(t.try_get_or_insert("overflow"), None);
    assert_eq!(t.len(), LOCAL_CAP);
    // …but an already-interned string still resolves at the cap.
    assert_eq!(t.try_get_or_insert("0"), Some(Sym(0)));
}

#[test]
fn builder_overflow_poisons_at_local_cap() {
    let mut b = SymbolTableBuilder::default();
    for i in 0..LOCAL_CAP as u32 {
        b.intern_or_poison(&i.to_string());
    }
    assert!(b.overflow().is_none());
    b.intern_or_poison("one too many");
    assert_eq!(b.overflow(), Some("class strings exceed 61,184"));
}

#[test]
fn archived_round_trip() {
    let t = built(&["", "gamma", "alpha", "beta", "alpha"]);
    let bytes = rkyv::to_bytes::<Error>(&t).unwrap();
    let archived = rkyv::access::<ArchivedSymbolTable, Error>(&bytes[..]).unwrap();

    let view = archived.view();
    for i in 0..t.len() {
        assert_eq!(
            view.get(Sym(i)),
            t.get(Sym(i)),
            "zero-copy get matches owned"
        );
    }
    for s in ["", "alpha", "beta", "gamma"] {
        assert_eq!(
            view.find(s),
            t.find(s),
            "zero-copy find matches owned: {s:?}"
        );
    }
    assert_eq!(view.find("absent"), None);
}

#[test]
fn rebuilt_drops_unused_and_matches_listrebuilder_numbering() {
    // Five symbols; two class lists reference a subset, leaving "b" (id 1) and "d" (id 3)
    // dropped. The rebuild must compact to {a,c,e} with new ids matching ListRebuilder.
    let t = built(&["a", "b", "c", "d", "e"]);
    let mut lists = ListVec::default();
    // list 0: a(0), e(4); list 1: c(2)
    let l0 = lists.new_list(0);
    assert!(lists.list_mut_at(l0).try_append(4));
    let _l1 = lists.new_list(2);

    let mut rb = ListRebuilder::new(lists.len(), t.len() as usize);
    rb.mark_list_used(&lists, l0);
    rb.mark_list_used(&lists, _l1);
    let rebuilt = rb.build(&lists);

    // Used old ids in ascending order: 0(a),2(c),4(e) → new dense ids 0,1,2.
    assert_eq!(rebuilt.value_reidx[0], Some(0));
    assert_eq!(rebuilt.value_reidx[1], None);
    assert_eq!(rebuilt.value_reidx[2], Some(1));
    assert_eq!(rebuilt.value_reidx[3], None);
    assert_eq!(rebuilt.value_reidx[4], Some(2));

    let compacted = t.rebuilt(&rebuilt.value_reidx);
    assert_eq!(compacted.len(), 3);
    assert_eq!(compacted.get(Sym(0)), "a");
    assert_eq!(compacted.get(Sym(1)), "c");
    assert_eq!(compacted.get(Sym(2)), "e");
    // find still works (permutation stayed content-sorted), dropped strings are gone.
    assert_eq!(compacted.find("a"), Some(Sym(0)));
    assert_eq!(compacted.find("c"), Some(Sym(1)));
    assert_eq!(compacted.find("e"), Some(Sym(2)));
    assert_eq!(compacted.find("b"), None);
    assert_eq!(compacted.find("d"), None);
}
