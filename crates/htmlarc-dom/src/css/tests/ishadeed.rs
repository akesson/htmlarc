use crate::{
    css::tests::helpers::select,
    dom::DomRead,
    html::{HtmlDoc, HtmlElement},
};

const BOOKS: &str = r##"
<div class="example-wrapper center">
    <p>Select the book that is next to the frame</p>
    <div>
        <div class="book" id="id1">book1</div>
        <div class="book" id="id2">book2</div>
        <div class="frame" id="id3">book3</div>
        <div class="book blue" id="id4">book4</div>
        <div class="book" id="id5">book5</div>
        <div class="book" id="id6">book6</div>
    </div>
</div>
"##;

#[test]
fn books_class() {
    assert_eq!(select(BOOKS, "div.frame"), ["div#id3.frame"]);
}

#[test]
fn books_next_sibling() {
    // https://ishadeed.com/article/css-has-guide/#adjacent-sibling-selector
    assert_eq!(select(BOOKS, ".frame + .book"), ["div#id4.book.blue"]);
}

#[test]
fn books_all_next_sibling() {
    // https://ishadeed.com/article/css-has-guide/#general-sibling-selector
    assert_eq!(
        select(BOOKS, ".frame ~ .book"),
        ["div#id4.book.blue", "div#id5.book", "div#id6.book"]
    );
}

#[test]
fn books_previous_sibling() {
    // https://ishadeed.com/article/css-has-guide/#the-previous-sibling-selector
    assert_eq!(select(BOOKS, ".book:has(+ .frame)"), ["div#id2.book"]);
}

#[test]
fn books_all_previous_sibling() {
    // https://ishadeed.com/article/css-has-guide/#the-previous-sibling-selector
    assert_eq!(
        select(BOOKS, ".book:has(~ .frame)"),
        ["div#id1.book", "div#id2.book"]
    );
}

#[test]
fn books_not_class() {
    // https://ishadeed.com/article/css-has-guide/#the-not-pseudo-class
    assert_eq!(
        select(BOOKS, ".book:not(.blue)"),
        [
            "div#id1.book",
            "div#id2.book",
            "div#id5.book",
            "div#id6.book"
        ]
    );
}

#[test]
fn card_has_img() {
    const CSS: &str = ".card:has(img)";

    const CARD_IMG: &str = r##"
    <div class="card">
      <img src="thumb.jpg" alt="" />
      <div class="card-content"></div>
    </div>"##;
    // https://ishadeed.com/article/css-has-guide/#card-with-image
    assert_eq!(select(CARD_IMG, CSS), ["div.card"]);

    const CARD_NO_IMG: &str = r##"
    <div class="card">
      <div class="card-content"></div>
    </div>"##;
    // https://ishadeed.com/article/css-has-guide/#card-without-an-image
    assert_eq!(select(CARD_NO_IMG, CSS), Vec::<String>::new());
}

#[test]
fn adjacent_sibling_and_has() {
    // https://ishadeed.com/article/css-has-guide/#adjacent-sibling-and-has

    let html = r#"
<div id="id1" class="shelf">
    <p></p>
</div>
<div id="id2" class="shelf">
    <div class="frame"></div>
    <div class="book-purple"></div>
</div>
    "#;

    assert_eq!(
        select(html, ".shelf:has(.frame + .book-purple)"),
        ["div#id2.shelf"]
    );
}

#[test]
fn parent_and_has() {
    // https://ishadeed.com/article/css-has-guide/#select-a-shelf-if-it-only-contains-a-box

    let html = r#"
<div id="id0" class="shelf">
    <section>
        <div class="box">
            <div></div>
        </div>
    </section>
</div>
<div id="id1" class="shelf">
    <div class="box">
        <p></p>
    </div>
</div>
<div id="id2" class="shelf">
    <div class="box">
        <div class="book"></div>
    </div>
</div>
<div id="id3" class="shelf">
    <section>
        <div class="box">
            <div class="book"></div>
        </div>
    </section>
</div>
    "#;

    assert_eq!(
        select(html, ".shelf:has(.box > .book)"),
        ["div#id2.shelf", "div#id3.shelf"]
    );
}

#[test]
fn not_and_has() {
    // https://ishadeed.com/article/css-has-guide/#select-a-box-without-a-blue-book

    let html = r#"
    <div class="box"></div>
    <div id="id1" class="box">
        <p class="red"></p>
    </div>
    <div id="id2" class="box">
        <p class="blue"></p>
    </div>
    <div id="id3" class="box">
        <p></p>
    </div>
    "#;

    assert_eq!(
        select(html, ".box:not(:has(.blue))"),
        ["div.box", "div#id1.box", "div#id3.box"]
    );
}

#[test]
fn nth_child_and_has() {
    // https://ishadeed.com/article/css-has-guide/#select-the-box-with-3-books

    let html = r#"
    <div class="box"></div>
    <div id="id1" class="box">
        <p class="book"></p>
    </div>
    <div id="id2" class="box">
        <p class="book"></p>
        <p class="book"></p>
    </div>
    <div id="id3" class="box">
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
    </div>
    <div id="id4" class="box">
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
    </div>
    "#;

    assert_eq!(
        select(html, ".box:has(.book:nth-last-child(n+3))"),
        ["div#id3.box", "div#id4.box"]
    );
}

#[test]
fn nth_last_child_and_has() {
    // https://ishadeed.com/article/css-has-guide/#add-spacing-to-each-3rd-items-if-5-books

    let html = r#"
    <div class="shelf"></div>
    <div id="id1" class="shelf">
        <p class="book"></p>
    </div>
    <div id="id2" class="shelf">
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
    </div>
    <div id="id3" class="shelf">
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
    </div>
    <div id="id4" class="shelf">
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
    </div>
    "#;

    assert_eq!(
        select(html, ".shelf:has(.book:nth-last-child(n+5))"),
        ["div#id2.shelf", "div#id4.shelf"]
    );
}

#[test]
fn nth_last_of_type_and_has() {
    // https://ishadeed.com/article/css-has-guide/#change-books-ordering

    let html = r#"
    <div class="shelf"></div>
    <div id="id1" class="shelf">
        <p class="book"></p>
        <span class="book"></span>
    </div>
    <div id="id2" class="shelf">
        <p class="book"></p>
        <p class="book"></p>
        <span class="book"></span>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
    </div>
    <div id="id3" class="shelf">
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <span class="book"></span>
        <span class="book"></span>
        <span class="book"></span>
    </div>
    <div id="id4" class="shelf">
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
        <p class="book"></p>
    </div>
    "#;

    assert_eq!(
        select(html, ".shelf:has(.book:nth-last-of-type(n+5))"),
        ["div#id2.shelf", "div#id4.shelf"]
    );
}
