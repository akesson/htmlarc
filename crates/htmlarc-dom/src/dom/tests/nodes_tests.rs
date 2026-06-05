use insta::assert_debug_snapshot;

use crate::dom::NodeIndex;
use crate::dom::nodes::Nodes;
use crate::html::HtmlTag;

use super::DomInner;

#[test]
fn test_nodecursor_debug() {
    let mut vec = Nodes::new();
    vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::head);
    let cursor = vec.add_as_last_child(NodeIndex::ROOT, HtmlTag::body);
    vec.add_as_first_child(cursor, HtmlTag::h1);

    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        2   sys_root
        [ 1]                  2                      0  > head
        [ 2]        1                   3        3   0  > body
        [ 3]                                         0  > 2  > h1
        "###);
}

#[test]
fn nodecursor_add_first_child() {
    let mut vec = Nodes::new();

    // if the parent node has no children, the new node is also the last child
    vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::body);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        1   sys_root
        [ 1]                                         0  > body
    "###);

    // if the parent node has children:
    // - the new node is the first child
    // - the new first child has the former first child as next sibling
    // - the former first child has the new node as prev sibling
    let i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::head);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            2        1   sys_root
        [ 1]        2                                0  > body
        [ 2]                  1                      0  > head
    "###);

    // can move cursor to first child
    assert_eq!(i, NodeIndex::new(2));
}

#[test]
fn nodecursor_add_last_child() {
    let mut vec = Nodes::new();

    // if the parent node has no children, the new node is also the first child
    vec.add_as_last_child(NodeIndex::ROOT, HtmlTag::head);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        1   sys_root
        [ 1]                                         0  > head
    "###);

    // if the parent node has children:
    // - the new node is the last child
    // - the new last child has the former last child as prev sibling
    // - the former last child has the new node as next sibling
    let i = vec.add_as_last_child(NodeIndex::ROOT, HtmlTag::body);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        2   sys_root
        [ 1]                  2                      0  > head
        [ 2]        1                                0  > body
    "###);

    // can move cursor to last child
    assert_eq!(i, NodeIndex::new(2));
}

#[test]
fn nodecursor_add_prev_sibling() {
    let mut vec = Nodes::new();

    let i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::html);

    // if the current node has no prev sibling, the new node is also the first child
    vec.add_as_prev_sibling(i, HtmlTag::DOCTYPE);

    println!("{:?}", vec);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            2        1   sys_root
        [ 1]        2                                0  > html
        [ 2]                  1                      0  > DOCTYPE
    "###);

    // if the current node has a prev sibling:
    // - the new node is the prev sibling
    // - the new prev sibling has the former prev sibling as prev sibling
    // - the new prev sibling has the current node as next sibling
    // - the former prev sibling has the new node as next sibling
    vec.add_as_prev_sibling(i, HtmlTag::sys_comment);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            2        1   sys_root
        [ 1]        3                                0  > html
        [ 2]                  3                      0  > DOCTYPE
        [ 3]        2         1                      0  > sys_comment
    "###);

    // can move vec to prev sibling
    assert_eq!(vec.prev_sibling_index(i), Some(NodeIndex::new(3)));
}

#[test]
fn nodecursor_add_next_sibling() {
    let mut vec = Nodes::new();

    let i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::sys_comment);

    // if the current node has no next sibling, the new node is also the last child
    vec.add_as_next_sibling(i, HtmlTag::html);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        2   sys_root
        [ 1]                  2                      0  > sys_comment
        [ 2]        1                                0  > html
    "###);

    // if the current node has a next sibling:
    // - the new node is the next sibling
    // - the new next sibling has the former next sibling as next sibling
    // - the new next sibling has the current node as prev sibling
    // - the former next sibling has the new node as prev sibling
    vec.add_as_next_sibling(i, HtmlTag::DOCTYPE);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        2   sys_root
        [ 1]                  3                      0  > sys_comment
        [ 2]        3                                0  > html
        [ 3]        1         2                      0  > DOCTYPE
    "###);

    // can move vec to next sibling
    assert_eq!(vec.next_sibling_index(i), Some(NodeIndex::new(3)));
}

#[test]
fn nodecursor_remove() {
    let mut vec = Nodes::new();

    // html
    //   head
    //   body
    //     main
    //        h1
    //        div
    //          h2
    //          textarea
    //        p

    let mut i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::html);
    _ = vec.add_as_first_child(i, HtmlTag::head);
    i = vec.add_as_last_child(i, HtmlTag::body);
    i = vec.add_as_first_child(i, HtmlTag::main);
    vec.add_as_first_child(i, HtmlTag::h1);
    i = vec.add_as_last_child(i, HtmlTag::div);
    vec.add_as_next_sibling(i, HtmlTag::p);
    vec.add_as_first_child(i, HtmlTag::h2);
    vec.add_as_last_child(i, HtmlTag::textarea);

    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1        1   sys_root
    [ 1]                            2        3   0  > html
    [ 2]                  3                      0  > 1  > head
    [ 3]        2                   4        4   0  > 1  > body
    [ 4]                            5        7   0  > 1  > 3  > main
    [ 5]                  6                      0  > 1  > 3  > 4  > h1
    [ 6]        5         7         8        9   0  > 1  > 3  > 4  > div
    [ 7]        6                                0  > 1  > 3  > 4  > p
    [ 8]                  9                      0  > 1  > 3  > 4  > 6  > h2
    [ 9]        8                                0  > 1  > 3  > 4  > 6  > textarea
    "###);

    // the removed node shouldn't reference any other node and shouldn't be referenced by any other node
    // if the node has children:
    // - the children shouldn't be referenced by nodes belonging to the tree
    let i = vec.remove(i);
    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1        1   sys_root
    [ 1]                            2        3   0  > html
    [ 2]                  3                      0  > 1  > head
    [ 3]        2                   4        4   0  > 1  > body
    [ 4]                            5        7   0  > 1  > 3  > main
    [ 5]                  7                      0  > 1  > 3  > 4  > h1
    [ 6]        5         7         8        9   div
    [ 7]        5                                0  > 1  > 3  > 4  > p
    [ 8]                  9                      6  > h2
    [ 9]        8                                6  > textarea
    "###);

    // the vec should be moved to the previous sibling
    assert_eq!(i, Some(NodeIndex::new(5)));

    // if the removed node is the first child:
    // - the next sibling should become the first child
    // - the removed node shouldn't be referenced by its former siblings
    let i = vec.remove(i.unwrap());
    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1        1   sys_root
    [ 1]                            2        3   0  > html
    [ 2]                  3                      0  > 1  > head
    [ 3]        2                   4        4   0  > 1  > body
    [ 4]                            7        7   0  > 1  > 3  > main
    [ 5]                  7                      h1
    [ 6]        5         7         8        9   div
    [ 7]                                         0  > 1  > 3  > 4  > p
    [ 8]                  9                      6  > h2
    [ 9]        8                                6  > textarea
    "###);

    // the vec should be moved to the next sibling
    assert_eq!(i, Some(NodeIndex::new(7)));

    // if the removed node is the literal last child:
    // - the removed node shouldn't be referenced by its parent
    let i = vec.remove(i.unwrap());
    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1        1   sys_root
    [ 1]                            2        3   0  > html
    [ 2]                  3                      0  > 1  > head
    [ 3]        2                   4        4   0  > 1  > body
    [ 4]                                         0  > 1  > 3  > main
    [ 5]                  7                      h1
    [ 6]        5         7         8        9   div
    [ 7]                                         p
    [ 8]                  9                      6  > h2
    [ 9]        8                                6  > textarea
    "###);

    // the vec should be moved to the parent
    assert_eq!(i, Some(NodeIndex::new(4)));

    let i = vec.parent_index(i.unwrap());

    // if the removed node is the last child:
    // - the previous sibling should become the last child
    vec.remove(i.unwrap());
    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1        1   sys_root
    [ 1]                            2        2   0  > html
    [ 2]                                         0  > 1  > head
    [ 3]        2                   4        4   body
    [ 4]                                         3  > main
    [ 5]                  7                      h1
    [ 6]        5         7         8        9   div
    [ 7]                                         p
    [ 8]                  9                      6  > h2
    [ 9]        8                                6  > textarea
    "###);
}

#[test]
fn nodecursor_unwrap() {
    let mut vec = Nodes::new();

    // body
    //   aside
    //     article
    //       h3
    //       textarea
    //     img
    //   main
    //     h1
    //     div
    //       h2
    //       input
    //       span
    //     p
    //   footer
    //     a
    //     section
    //       button

    let mut i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::body); // 1
    i = vec.add_as_first_child(i, HtmlTag::aside); // 2
    i = vec.add_as_first_child(i, HtmlTag::article); // 3
    vec.add_as_first_child(i, HtmlTag::h3); // 4
    vec.add_as_last_child(i, HtmlTag::textarea); // 5
    vec.add_as_next_sibling(i, HtmlTag::img); // 6
    let root_idx = vec.parent_index(i);
    assert_eq!(root_idx, Some(NodeIndex::new(2)));
    let mut i = root_idx.unwrap();
    i = vec.add_as_next_sibling(i, HtmlTag::main); // 7
    vec.add_as_first_child(i, HtmlTag::h1); // 8
    i = vec.add_as_last_child(i, HtmlTag::div); // 9
    vec.add_as_first_child(i, HtmlTag::h2); // 10
    vec.add_as_last_child(i, HtmlTag::input); // 11
    vec.add_as_last_child(i, HtmlTag::span); // 12
    vec.add_as_next_sibling(i, HtmlTag::p); // 13

    let i = vec.parent_index(i);
    assert_eq!(i, Some(NodeIndex::new(7)));
    let mut i = vec.add_as_next_sibling(i.unwrap(), HtmlTag::footer); // 14
    vec.add_as_first_child(i, HtmlTag::a); // 15
    i = vec.add_as_last_child(i, HtmlTag::section); // 16
    vec.add_as_first_child(i, HtmlTag::button); // 17

    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1        1   sys_root
    [ 1]                            2       14   0  > body
    [ 2]                  7         3        6   0  > 1  > aside
    [ 3]                  6         4        5   0  > 1  > 2  > article
    [ 4]                  5                      0  > 1  > 2  > 3  > h3
    [ 5]        4                                0  > 1  > 2  > 3  > textarea
    [ 6]        3                                0  > 1  > 2  > img
    [ 7]        2        14         8       13   0  > 1  > main
    [ 8]                  9                      0  > 1  > 7  > h1
    [ 9]        8        13        10       12   0  > 1  > 7  > div
    [10]                 11                      0  > 1  > 7  > 9  > h2
    [11]       10        12                      0  > 1  > 7  > 9  > input
    [12]       11                                0  > 1  > 7  > 9  > span
    [13]        9                                0  > 1  > 7  > p
    [14]        7                  15       16   0  > 1  > footer
    [15]                 16                      0  > 1  > 14 > a
    [16]       15                  17       17   0  > 1  > 14 > section
    [17]                                         0  > 1  > 14 > 16 > button
    "###);

    // the unwrapped node shouldn't reference any other node in the tree
    // if the unwraped node is the last child of its parent:
    // - the last child of the unwrapped node should become the last child of the parent
    let i = vec.unwrap_node(i);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        1   sys_root
        [ 1]                            2       14   0  > body
        [ 2]                  7         3        6   0  > 1  > aside
        [ 3]                  6         4        5   0  > 1  > 2  > article
        [ 4]                  5                      0  > 1  > 2  > 3  > h3
        [ 5]        4                                0  > 1  > 2  > 3  > textarea
        [ 6]        3                                0  > 1  > 2  > img
        [ 7]        2        14         8       13   0  > 1  > main
        [ 8]                  9                      0  > 1  > 7  > h1
        [ 9]        8        13        10       12   0  > 1  > 7  > div
        [10]                 11                      0  > 1  > 7  > 9  > h2
        [11]       10        12                      0  > 1  > 7  > 9  > input
        [12]       11                                0  > 1  > 7  > 9  > span
        [13]        9                                0  > 1  > 7  > p
        [14]        7                  15       17   0  > 1  > footer
        [15]                 17                      0  > 1  > 14 > a
        [16]                                         section
        [17]       15                                0  > 1  > 14 > button
    "###);

    // the vec should be on the unwrapped node's first child
    assert_eq!(i, Some(NodeIndex::new(17)));

    let mut i = vec.parent_index(i.unwrap()).unwrap();
    i = vec.prev_sibling_index(i).unwrap();
    i = vec.first_child_index(i).unwrap();
    i = vec.next_sibling_index(i).unwrap();

    // if the unwraped node has siblings:
    // - the unwrapped node's first child should become the next sibling of the unwraped node's previous sibling
    // - the unwrapped node's last child should become the previous sibling of the unwraped node's next sibling
    // - the unwrapped node's children should become the children of its parent
    let i = vec.unwrap_node(i);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        1   sys_root
        [ 1]                            2       14   0  > body
        [ 2]                  7         3        6   0  > 1  > aside
        [ 3]                  6         4        5   0  > 1  > 2  > article
        [ 4]                  5                      0  > 1  > 2  > 3  > h3
        [ 5]        4                                0  > 1  > 2  > 3  > textarea
        [ 6]        3                                0  > 1  > 2  > img
        [ 7]        2        14         8       13   0  > 1  > main
        [ 8]                 10                      0  > 1  > 7  > h1
        [ 9]                                         div
        [10]        8        11                      0  > 1  > 7  > h2
        [11]       10        12                      0  > 1  > 7  > input
        [12]       11        13                      0  > 1  > 7  > span
        [13]       12                                0  > 1  > 7  > p
        [14]        7                  15       17   0  > 1  > footer
        [15]                 17                      0  > 1  > 14 > a
        [16]                                         section
        [17]       15                                0  > 1  > 14 > button
    "###);

    // the vec should be on the unwrapped node's first child
    assert_eq!(i, Some(NodeIndex::new(10)));

    let mut i = vec.parent_index(i.unwrap()).unwrap();
    i = vec.prev_sibling_index(i).unwrap();
    i = vec.first_child_index(i).unwrap();

    // if the unwraped node is the first child of its parent:
    // - the unwraped node's first child should become the first child of the unwraped node's parent
    let i = vec.unwrap_node(i);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        1   sys_root
        [ 1]                            2       14   0  > body
        [ 2]                  7         4        6   0  > 1  > aside
        [ 3]                                         article
        [ 4]                  5                      0  > 1  > 2  > h3
        [ 5]        4         6                      0  > 1  > 2  > textarea
        [ 6]        5                                0  > 1  > 2  > img
        [ 7]        2        14         8       13   0  > 1  > main
        [ 8]                 10                      0  > 1  > 7  > h1
        [ 9]                                         div
        [10]        8        11                      0  > 1  > 7  > h2
        [11]       10        12                      0  > 1  > 7  > input
        [12]       11        13                      0  > 1  > 7  > span
        [13]       12                                0  > 1  > 7  > p
        [14]        7                  15       17   0  > 1  > footer
        [15]                 17                      0  > 1  > 14 > a
        [16]                                         section
        [17]       15                                0  > 1  > 14 > button
    "###);

    // the vec should be on the unwraped node's first child
    assert_eq!(i, Some(NodeIndex::new(4)));

    // body
    //   div
    //   section

    let mut vec = Nodes::new();
    let mut i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::body);
    i = vec.add_as_first_child(i, HtmlTag::div);
    i = vec.add_as_next_sibling(i, HtmlTag::section);
    vec.unwrap_node(i);
    // when unwrapping an empty element, its previous sibling should have no next sibling and become the last child of the parent
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        1   sys_root
        [ 1]                            2        2   0  > body
        [ 2]                                         0  > 1  > div
        [ 3]                                         section
    "###);

    let mut vec = Nodes::new();
    let mut i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::body);
    i = vec.add_as_first_child(i, HtmlTag::div);
    vec.add_as_next_sibling(i, HtmlTag::section);
    vec.unwrap_node(i);
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        1   sys_root
        [ 1]                            3        3   0  > body
        [ 2]                                         div
        [ 3]                                         0  > 1  > section
    "###);
}

#[test]
fn nodecursor_replace() {
    let mut vec = Nodes::new();

    // nav
    //   img
    // div
    //   a
    //   section
    // main
    //   h1
    //   p
    //   textarea
    // footer
    //   header
    //     h2

    let mut i = vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::nav); // 1
    vec.add_as_first_child(i, HtmlTag::img); // 2
    i = vec.add_as_next_sibling(i, HtmlTag::div); // 3
    vec.add_as_first_child(i, HtmlTag::a); // 4
    vec.add_as_last_child(i, HtmlTag::section); // 5
    i = vec.add_as_next_sibling(i, HtmlTag::main); // 6
    vec.add_as_first_child(i, HtmlTag::h1); // 7
    vec.add_as_last_child(i, HtmlTag::p); // 8
    vec.add_as_last_child(i, HtmlTag::textarea); // 9
    i = vec.add_as_next_sibling(i, HtmlTag::footer); // 10
    i = vec.add_as_first_child(i, HtmlTag::header); // 11
    vec.add_as_first_child(i, HtmlTag::h2); // 12
    assert_eq!(i, NodeIndex::new(11));
    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1       10   sys_root
    [ 1]                  3         2        2   0  > nav
    [ 2]                                         0  > 1  > img
    [ 3]        1         6         4        5   0  > div
    [ 4]                  5                      0  > 3  > a
    [ 5]        4                                0  > 3  > section
    [ 6]        3        10         7        9   0  > main
    [ 7]                  8                      0  > 6  > h1
    [ 8]        7         9                      0  > 6  > p
    [ 9]        8                                0  > 6  > textarea
    [10]        6                  11       11   0  > footer
    [11]                           12       12   0  > 10 > header
    [12]                                         0  > 10 > 11 > h2
    "###);
    i = vec.parent_index(i).unwrap();
    i = vec.prev_sibling_index(i).unwrap();
    i = vec.prev_sibling_index(i).unwrap();

    assert_debug_snapshot!(vec, @r###"
    idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
    [ 0]                            1       10   sys_root
    [ 1]                  3         2        2   0  > nav
    [ 2]                                         0  > 1  > img
    [ 3]        1         6         4        5   0  > div
    [ 4]                  5                      0  > 3  > a
    [ 5]        4                                0  > 3  > section
    [ 6]        3        10         7        9   0  > main
    [ 7]                  8                      0  > 6  > h1
    [ 8]        7         9                      0  > 6  > p
    [ 9]        8                                0  > 6  > textarea
    [10]        6                  11       11   0  > footer
    [11]                           12       12   0  > 10 > header
    [12]                                         0  > 10 > 11 > h2
    "###);

    // if the substitute node had siblings:
    // - update the substitute node's sibling references
    vec.replace_with(i, NodeIndex::new(6));
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1       10   sys_root
        [ 1]                  6         2        2   0  > nav
        [ 2]                                         0  > 1  > img
        [ 3]                            4        5   div
        [ 4]                  5                      3  > a
        [ 5]        4                                3  > section
        [ 6]        1        10         7        9   0  > main
        [ 7]                  8                      0  > 6  > h1
        [ 8]        7         9                      0  > 6  > p
        [ 9]        8                                0  > 6  > textarea
        [10]        6                  11       11   0  > footer
        [11]                           12       12   0  > 10 > header
        [12]                                         0  > 10 > 11 > h2
    "###);

    // if the substitute node has no siblings:
    // - update the substitute node's parent reference
    vec.replace_with(NodeIndex::new(6), NodeIndex::new(11));
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1       10   sys_root
        [ 1]                 11         2        2   0  > nav
        [ 2]                                         0  > 1  > img
        [ 3]                            4        5   div
        [ 4]                  5                      3  > a
        [ 5]        4                                3  > section
        [ 6]                            7        9   main
        [ 7]                  8                      6  > h1
        [ 8]        7         9                      6  > p
        [ 9]        8                                6  > textarea
        [10]       11                                0  > footer
        [11]        1        10        12       12   0  > header
        [12]                                         0  > 11 > h2
    "###);

    // if the replacement node is the first child of its parent:
    // - the substitute node should become the first child of the parent
    vec.replace_with(NodeIndex::new(11), NodeIndex::new(1));
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1       10   sys_root
        [ 1]                 10         2        2   0  > nav
        [ 2]                                         0  > 1  > img
        [ 3]                            4        5   div
        [ 4]                  5                      3  > a
        [ 5]        4                                3  > section
        [ 6]                            7        9   main
        [ 7]                  8                      6  > h1
        [ 8]        7         9                      6  > p
        [ 9]        8                                6  > textarea
        [10]        1                                0  > footer
        [11]                           12       12   header
        [12]                                         11 > h2
    "###);

    // if the replacement node is the last child of its parent:
    // - the substitute node should become the last child of the parent
    vec.replace_with(NodeIndex::new(1), NodeIndex::new(10));
    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                           10       10   sys_root
        [ 1]                            2        2   nav
        [ 2]                                         1  > img
        [ 3]                            4        5   div
        [ 4]                  5                      3  > a
        [ 5]        4                                3  > section
        [ 6]                            7        9   main
        [ 7]                  8                      6  > h1
        [ 8]        7         9                      6  > p
        [ 9]        8                                6  > textarea
        [10]                                         0  > footer
        [11]                           12       12   header
        [12]                                         11 > h2
    "###);
}

#[test]
fn test_nodecursor_text() {
    let mut vec = Nodes::new();
    vec.add_as_first_child(NodeIndex::ROOT, HtmlTag::head);
    let i = vec.add_as_last_child(NodeIndex::ROOT, HtmlTag::body);
    vec.add_as_first_child(i, HtmlTag::h1);

    assert_debug_snapshot!(vec, @r###"
        idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag
        [ 0]                            1        2   sys_root
        [ 1]                  2                      0  > head
        [ 2]        1                   3        3   0  > body
        [ 3]                                         0  > 2  > h1
        "###);
}

#[test]
fn test_attrs() {
    let mut inner = DomInner::default();
    let div_a = add_div_and_class(&mut inner, NodeIndex::ROOT, "a");

    assert_eq!(inner.nodes.attr_list_index(div_a), None);
}

#[cfg(test)]
pub fn add_div_and_class(inner: &mut DomInner, index: NodeIndex, classes: &str) -> NodeIndex {
    let div = inner.nodes.add_as_last_child(index, HtmlTag::div);
    inner.add_classes(div, classes);
    div
}
