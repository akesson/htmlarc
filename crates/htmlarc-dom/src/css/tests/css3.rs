use crate::css::tests::helpers::{select, try_select};

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-1.html
#[test]
fn w3c_group_of_selectors() {
    let html = r#"
<ul>
    <li id="li1">The background of this list item should be green</li>
    <li id="li2">The background of this second list item should be also green</li>
</ul>
<p id="p1">The background of this paragraph should be green.</p>
"#;
    assert_eq!(select(html, "ul li, p"), ["li#li1", "li#li2", "p#p1"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-2.html
#[test]
fn w3c_element_selector() {
    let html = r#"
<address id="green">This address element should have a green background.</address>
"#;
    assert_eq!(select(html, "address"), ["address#green"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-5.html
#[test]
fn w3c_attribute_existence_selector() {
    let html = r#"
<p title="title" id="titled">This paragraph should have a green background because its TITLE attribute is set.</p>
"#;

    assert_eq!(select(html, "p[title]"), ["p#titled"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-6.html
#[test]
fn w3c_attribute_value_selector() {
    let html = r#"
<address title="foo" id="green">
<span title="b">This line should </span>
    <span title="aa">have a green background.
</span>
</address>
"#;

    assert_eq!(select(html, r#"address[title="foo"]"#), ["address#green"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-7.html
#[test]
fn w3c_attribute_multivalue_selector() {
    let html = r#"
<p id="id1" class="a b c">This paragraph should have green background because CLASS contains &quot;b&quot;</p>
<address id="id2" title="tot foo bar">
<span class="a c">This address should also</span>
    <span class="a bb c">have green background because the selector in the last rule does not apply to the inner SPANs.</span>
</address>
"#;

    assert_eq!(select(html, r#"address[title~="foo"]"#), ["address#id2"]);
    assert_eq!(select(html, r#"p[class~="b"]"#), ["p#id1.a.b.c"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-7b.html
#[test]
fn w3c_attribute_multivalue_selector_b() {
    let html = r#"
<p id="id1" title="hello world">This line should have a green background.</p>
"#;

    assert!(select(html, r#"[title~="hello world"]"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-8.html
#[test]
fn w3c_hyphen_separated_attribute_value_selector() {
    let html = r#"
<p id="id1" lang="en-gb">This paragraph should have green background because its language is &quot;en-gb&quot;</p>
<address id="id2" lang="fi">
<span lang="en-us">This address should also</span>
    <span lang="en-fr">have green background because the language of the inner SPANs is not French.</span>
</address>
"#;

    assert_eq!(select(html, r#"p[lang|="en"]"#), ["p#id1"]);
    assert_eq!(select(html, r#"address[lang="fi"]"#), ["address#id2"]);
    assert!(select(html, r#"span[lang|="fr"]"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-9.html
#[test]
fn w3c_substring_matching_attribute_selector_start() {
    let html = r#"
<p id="id1" title="foobar">This paragraph should have a green background<br>
because its title attribute begins with &quot;foo&quot;</p>
"#;

    assert_eq!(select(html, r#"p[title^="foo"]"#), ["p#id1"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-10.html
#[test]
fn w3c_substring_matching_attribute_selector_end() {
    let html = r#"
<p id="id1" title="foobar">This paragraph should have a green background because
its title attribute ends with &quot;bar&quot;</p>
"#;

    assert_eq!(select(html, r#"p[title$="bar"]"#), ["p#id1"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-11.html
#[test]
fn w3c_substring_matching_attribute_selector_contains() {
    let html = r#"
<p id="id1" title="foobarufoo">This paragraph should have a green background because
its title attribute contains &quot;bar&quot;</p>
"#;

    assert_eq!(select(html, r#"p[title*="bar"]"#), ["p#id1"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-13.html
#[test]
fn w3c_class_selector() {
    let html = r#"
<ul>
    <li id="id1" class="t1">This list item should have green background because its class is &quot;t1&quot;</li>
    <li id="id2" class="t2">This list item should have green background because its class is &quot;t2&quot;</li>
    <li id="id3" class="t2">
<span class="t33">This list item should have green background because the inner SPAN does not match SPAN.t3</span>
</li>
</ul>
"#;

    assert_eq!(select(html, r#".t1"#), ["li#id1.t1"]);
    assert_eq!(select(html, r#"li.t2"#), ["li#id2.t2", "li#id3.t2"]);
    assert!(select(html, ".t3").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-14.html
#[test]
fn w3c_more_than_one_class_selector() {
    let html = r#"
<p class="t1 t2">This paragraph
should have a green background and a green thick solid border because
it carries both classes t1 and t2.</p>

<div class="test">This line
should be green.</div>
"#;

    assert_eq!(select(html, r#"p.t1"#), ["p.t1.t2"]);
    assert_eq!(select(html, r#"p.t2"#), ["p.t1.t2"]);
    assert!(select(html, "div.teST").is_empty());
    assert!(select(html, "div.te").is_empty());
    assert!(select(html, "div.st").is_empty());
    assert!(select(html, "div.te.st").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-14b.html
#[test]
fn w3c_more_than_one_class_selector_b() {
    let html = r#"
<p class="t1">This line should be green.</p>
<p class="t1 t2">This line should be green.</p>
"#;

    assert!(select(html, ".t1.fail").is_empty());
    assert!(select(html, ".fail.t1").is_empty());
    assert!(select(html, ".t2.fail").is_empty());
    assert!(select(html, ".fail.t2").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-14c.html
#[test]
fn w3c_more_than_one_class_selector_c() {
    let html = r#"
<p class="t1 t2">This line should be green.</p>
<div class="t3">This line should be green.</div>
<address class="t4 t5 t6">This line should be green.</address>
"#;

    assert_eq!(select(html, r#"p.t1.t2"#), ["p.t1.t2"]);
    assert_eq!(select(html, r#"address.t5.t5"#), ["address.t4.t5.t6"]);
    assert!(select(html, "div.t1").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-14d.html
#[test]
fn w3c_negated_more_than_one_class_selector() {
    let html = r#"
<p class="t1 t2">This line should be green.</p>
"#;

    assert!(select(html, ".t1:not(.t2)").is_empty());
    assert!(select(html, ":not(.t2).t1").is_empty());
    assert!(select(html, ".t2:not(.t1)").is_empty());
    assert!(select(html, ":not(.t1).t2").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-14e.html
#[test]
fn w3c_negated_more_than_one_class_selector_b() {
    let html = r#"
<p class="t1 t2">This line should be green.</p>
<div class="t3">This line should be green.</div>
<address class="t4 t5 t6">This line should be green.</address>
"#;

    assert_eq!(select(html, r#"div:not(.t1)"#), ["div.t3"]);
    assert!(select(html, "p:not(.t1):not(.t2)").is_empty());
    assert!(select(html, "address:not(.t5):not(.t5)").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-15.html
#[test]
fn w3c_id_selector() {
    let html = r#"
<ul>
    <li id="t1">This list item should have a green background. because its ID is &quot;t1&quot;</li>
    <li id="t2">This list item should have a green background. because its ID is &quot;t2&quot;</li>
    <li id="t3"><span id="t44">This list item should have a green background. because the inner SPAN does not match &quot;#t4&quot;</span></li>
</ul>
"#;

    assert_eq!(select(html, r#"#t1"#), ["li#t1"]);
    assert_eq!(select(html, r#"li#t2"#), ["li#t2"]);
    assert_eq!(select(html, r#"li#t3"#), ["li#t3"]);
    assert!(select(html, "#t4").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-15b.html
#[test]
fn w3c_multiple_id_selector() {
    let html = r#"
<p id="test">This line should be green.</p>
<div id="pass">This line should be green.</div>
"#;

    assert_eq!(select(html, "#pass#pass"), ["div#pass"]);
    assert!(try_select(html, "#test#fail").is_err());
    assert!(try_select(html, "#fail#test").is_err());
    assert!(select(html, "#fail").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-27.html
#[test]
fn w3c_root_pseudo_class() {
    // tested in css/selectors/pseudo_class.rs#L500
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-27a.html
#[test]
fn w3c_impossible_rules() {
    let html = r#"
<p>This line should be green (there should be no red on this page).</p>
"#;

    assert!(select(html, ":root:first-child").is_empty());
    assert!(select(html, ":root:last-child").is_empty());
    assert!(select(html, ":root:only-child").is_empty());
    assert!(select(html, ":root:nth-child(1)").is_empty());
    assert!(select(html, ":root:nth-child(n)").is_empty());
    assert!(select(html, ":root:nth-last-child(1)").is_empty());
    assert!(select(html, ":root:nth-last-child(n)").is_empty());
    assert!(select(html, ":root:first-of-type").is_empty());
    assert!(select(html, ":root:last-of-type").is_empty());
    assert!(select(html, ":root:only-of-type").is_empty());
    assert!(select(html, ":root:nth-of-type(1)").is_empty());
    assert!(select(html, ":root:nth-of-type(n)").is_empty());
    assert!(select(html, ":root:nth-last-of-type(1)").is_empty());
    assert!(select(html, ":root:nth-last-of-type(n)").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-28.html
#[test]
fn w3c_nth_child_pseudo_class() {
    let html = r#"
<ul>
  <li class="red">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="red">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="red">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ul>
<ol>
  <li>First list item</li>
  <li class="red">This second list item should have a green background</li>
  <li>Third list item</li>
  <li class="red">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="red">This sixth list item should have a green background</li>
</ol>
<div>
<table border="1" class="t1">
  <tr class="red">
<td>Green row : 1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr class="red">
<td>Green row : 2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr class="red">
<td>Green row : 3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr class="red">
<td>Green row : 4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr>
<td>5.1</td>
<td>5.2</td>
<td>5.3</td>
</tr>
  <tr>
<td>6.1</td>
<td>6.2</td>
<td>6.3</td>
</tr>
</table>

<table class="t2" border="1">
  <tr>
<td class="red">green cell</td>
<td>1.2</td>
<td>1.3</td>
      <td class="red">green cell</td>
<td>1.5</td>
<td>1.6</td>
      <td class="red">green cell</td>
<td>1.8</td>
</tr>
  <tr>
<td class="red">green cell</td>
<td>2.2</td>
<td>2.3</td>
      <td class="red">green cell</td>
<td>2.5</td>
<td>2.6</td>
      <td class="red">green cell</td>
<td>2.8</td>
</tr>
  <tr>
<td class="red">green cell</td>
<td>3.2</td>
<td>3.3</td>
      <td class="red">green cell</td>
<td>3.5</td>
<td>3.6</td>
      <td class="red">green cell</td>
<td>3.8</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, "ul > li:nth-child(odd)"),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, "ol > li:nth-child(even)"),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, "table.t1 tr:nth-child(-n+4)"),
        ["tr.red", "tr.red", "tr.red", "tr.red"]
    );
    assert_eq!(
        select(html, "table.t2 td:nth-child(3n+1)"),
        [
            "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red",
            "td.red"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-28b.html
#[test]
fn w3c_nth_child_pseudo_class_b() {
    let html = r#"
<ul>
  <li class="green">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="green">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="green">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ul>
<ol>
  <li>First list item</li>
  <li class="green">This second list item should have a green background</li>
  <li>Third list item</li>
  <li class="green">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="green">This sixth list item should have a green background</li>
</ol>
<div>
<table border="1" class="t1">
  <tr class="green">
<td>Green row : 1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr class="green">
<td>Green row : 2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr class="green">
<td>Green row : 3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr class="green">
<td>Green row : 4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr>
<td>5.1</td>
<td>5.2</td>
<td>5.3</td>
</tr>
  <tr>
<td>6.1</td>
<td>6.2</td>
<td>6.3</td>
</tr>
</table>
<p></p>
<table class="t2" border="1">
  <tr>
<td class="green">green cell</td>
<td>1.2</td>
<td>1.3</td>
      <td class="green">green cell</td>
<td>1.5</td>
<td>1.6</td>
      <td class="green">green cell</td>
<td>1.8</td>
</tr>
  <tr>
<td class="green">green cell</td>
<td>2.2</td>
<td>2.3</td>
      <td class="green">green cell</td>
<td>2.5</td>
<td>2.6</td>
      <td class="green">green cell</td>
<td>2.8</td>
</tr>
  <tr>
<td class="green">green cell</td>
<td>3.2</td>
<td>3.3</td>
      <td class="green">green cell</td>
<td>3.5</td>
<td>3.6</td>
      <td class="green">green cell</td>
<td>3.8</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, "ul > li:nth-child(odd)"),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, "ol > li:nth-child(even)"),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, "table.t1 tr:nth-child(-n+4)"),
        ["tr.green", "tr.green", "tr.green", "tr.green"]
    );
    assert_eq!(
        select(html, "table.t2 td:nth-child(3n+1)"),
        [
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
            "td.green", "td.green"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-29.html
#[test]
fn w3c_nth_last_child_pseudo_class() {
    let html = r#"
<ul>
  <li>First list item</li>
  <li class="red">This second list item should have a green background</li>
  <li>Third list item</li>
  <li class="red">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="red">This sixth list item should have a green background</li>
</ul>
<ol>
  <li class="red">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="red">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="red">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ol>
<div>
<table border="1" class="t1">
  <tr>
<td>1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr>
<td>2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr class="red">
<td>Green row : 3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr class="red">
<td>Green row : 4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr class="red">
<td>Green row : 5.1</td>
<td>5.2</td>
      <td>5.3</td>
</tr>
  <tr class="red">
<td>Green row : 6.1</td>
<td>6.2</td>
      <td>6.3</td>
</tr>
</table>
<p></p>
<table class="t2" border="1">
  <tr>
<td>1.1</td>
<td class="red">green cell</td>
<td>1.3</td>
      <td>1.4</td>
<td class="red">green cell</td>
<td>1.6</td>
      <td>1.7</td>
<td class="red">green cell</td>
</tr>
  <tr>
<td>2.1</td>
<td class="red">green cell</td>
<td>2.3</td>
      <td>2.4</td>
<td class="red">green cell</td>
<td>2.6</td>
      <td>2.7</td>
<td class="red">green cell</td>
</tr>
  <tr>
<td>3.1</td>
<td class="red">green cell</td>
<td>3.3</td>
      <td>3.4</td>
<td class="red">green cell</td>
<td>3.6</td>
      <td>3.7</td>
<td class="red">green cell</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, "ul > li:nth-last-child(odd)"),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, "ol > li:nth-last-child(even)"),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, "table.t1 tr:nth-last-child(-n+4)"),
        ["tr.red", "tr.red", "tr.red", "tr.red"]
    );
    assert_eq!(
        select(html, "table.t2 td:nth-last-child(3n+1)"),
        [
            "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red",
            "td.red"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-29b.html
#[test]
fn w3c_nth_last_child_pseudo_class_b() {
    let html = r#"
<ul>
  <li>First list item</li>
  <li class="green">This second list item should have a green background</li>
  <li>Third list item</li>
  <li class="green">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="green">This sixth list item should have a green background</li>
</ul>
<ol>
  <li class="green">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="green">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="green">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ol>
<div>
<table border="1" class="t1">
  <tr>
<td>1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr>
<td>2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr class="green">
<td>Green row : 3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr class="green">
<td>Green row : 4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr class="green">
<td>Green row : 5.1</td>
<td>5.2</td>
      <td>5.3</td>
</tr>
  <tr class="green">
<td>Green row : 6.1</td>
<td>6.2</td>
      <td>6.3</td>
</tr>
</table>
<p></p>
<table class="t2" border="1">
  <tr>
<td>1.1</td>
<td class="green">green cell</td>
<td>1.3</td>
      <td>1.4</td>
<td class="green">green cell</td>
<td>1.6</td>
      <td>1.7</td>
<td class="green">green cell</td>
</tr>
  <tr>
<td>2.1</td>
<td class="green">green cell</td>
<td>2.3</td>
      <td>2.4</td>
<td class="green">green cell</td>
<td>2.6</td>
      <td>2.7</td>
<td class="green">green cell</td>
</tr>
  <tr>
<td>3.1</td>
<td class="green">green cell</td>
<td>3.3</td>
      <td>3.4</td>
<td class="green">green cell</td>
<td>3.6</td>
      <td>3.7</td>
<td class="green">green cell</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, "ul > li:nth-last-child(odd)"),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, "ol > li:nth-last-child(even)"),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, "table.t1 tr:nth-last-child(-n+4)"),
        ["tr.green", "tr.green", "tr.green", "tr.green"]
    );
    assert_eq!(
        select(html, "table.t2 td:nth-last-child(3n+1)"),
        [
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
            "td.green", "td.green"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-30.html
#[test]
fn w3c_nth_of_type_pseudo_class() {
    let html = r#"
<p>This paragraph is here only to fill space in the DOM</p>
<address>And this address too..</address>
<p>So does this paragraph !</p>
<p class="red">But this one should have green background</p>
<dl>
  <dt class="red">First definition term that should have green background</dt>
    <dd class="red">First definition that should have green background</dd>
  <dt>Second definition term</dt>
    <dd>Second definition</dd>
  <dt>Third definition term</dt>
    <dd>Third definition</dd>
  <dt class="red">Fourth definition term that should have green background</dt>
    <dd class="red">Fourth definition that should have green background</dd>
  <dt>Fifth definition term</dt>
    <dd>Fifth definition</dd>
  <dt>Sixth definition term</dt>
    <dd>Sixth definition</dd>
</dl>
"#;

    assert_eq!(select(html, "p:nth-of-type(3)"), ["p.red"]);
    assert_eq!(
        select(html, "dl > :nth-of-type(3n+1)"),
        ["dt.red", "dd.red", "dt.red", "dd.red"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-31.html
#[test]
fn w3c_nth_last_of_type_pseudo_class() {
    let html = r#"
<p class="red">This paragraph should have green background</p>
<address>But this address is here only to fill space in the dom..</address>
<p>So does this paragraph !</p>
<p>And so does this one too.</p>
<dl>
  <dt>First definition term</dt>
    <dd>First definition</dd>
  <dt>Second definition term</dt>
    <dd>Second definition</dd>
  <dt class="red">Third definition term that should have green background</dt>
    <dd class="red">Third definition that should have green background</dd>
  <dt>Fourth definition term</dt>
    <dd>Fourth definition</dd>
  <dt>Fifth definition term</dt>
    <dd>Fifth definition</dd>
  <dt class="red">Sixth definition term that should have green background</dt>
    <dd class="red">Sixth definition that should have green background</dd>
</dl>
"#;

    assert_eq!(select(html, "p:nth-last-of-type(3)"), ["p.red"]);
    assert_eq!(
        select(html, "dl > :nth-last-of-type(3n+1)"),
        ["dt.red", "dd.red", "dt.red", "dd.red"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-32.html
#[test]
fn w3c_first_child_pseudo_class() {
    let html = r#"
<div>
<table class="t1" border="1">
  <tr>
    <td class="red">green cell</td>
    <td>1.2</td>
    <td>1.3</td>
  </tr>
  <tr>
    <td class="red">green cell</td>
    <td>2.2</td>
    <td>2.3</td>
  </tr>
  <tr>
    <td class="red">green cell</td>
    <td>3.2</td>
    <td>3.3</td>
  </tr>
</table>
</div>
<p>This paragraph contains some text
          <span>and a span that should have a green background</span>
</p>
"#;

    assert_eq!(
        select(html, ".t1 td:first-child"),
        ["td.red", "td.red", "td.red"]
    );
    assert_eq!(select(html, "p > :first-child"), ["span"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-33.html
#[test]
fn w3c_last_child_pseudo_class() {
    let html = r#"
<div>
<table class="t1" border="1">
  <tr>
    <td>1.1</td>
    <td>1.2</td>
    <td class="red">green cell</td>
  </tr>
  <tr>
    <td>2.1</td>
    <td>2.2</td>
    <td class="red">green cell</td>
  </tr>
  <tr>
    <td>3.1</td>
    <td>3.2</td>
    <td class="red">green cell</td>
  </tr>
</table>
</div>
<p>
<span>This paragraph contains a span that should
     have a green background</span> and some text after it.</p>
"#;

    assert_eq!(
        select(html, ".t1 td:last-child"),
        ["td.red", "td.red", "td.red"]
    );
    assert_eq!(select(html, "p > :last-child"), ["span"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-34.html
#[test]
fn w3c_first_of_type_pseudo_class() {
    let html = r#"
<div>This div contains 3 addresses:
<address class="red">A first address that should have a green background</address>
<address>A second address with normal background</address>
<address>A third address with normal background</address>
</div>
"#;

    assert_eq!(select(html, "address:first-of-type"), ["address.red"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-35.html
#[test]
fn w3c_last_of_type_pseudo_class() {
    let html = r#"
<div>
<address>A first address with normal background</address>
<address>A second address with normal background</address>
<address class="red">A third address that should have a green background</address>
This div contains 3 addresses above this sentence.</div>
"#;

    assert_eq!(select(html, "address:last-of-type"), ["address.red"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-36.html
#[test]
fn w3c_only_child_pseudo_class() {
    let html = r#"
<p>This paragraph should have normal background</p>
<div>This div contains only one paragraph
    <p class="red">This paragraph should have green background</p>
</div>
"#;

    assert_eq!(select(html, "p:only-child"), ["p.red"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-37.html
#[test]
fn w3c_only_of_type_pseudo_class() {
    let html = r#"
<div class="t1">
<p>This paragraph should have normal background</p>
<address class="red">But this address should have green background</address>
<p>This paragraph should have normal background</p>
</div>
"#;

    assert_eq!(select(html, ".t1 :only-of-type"), ["address.red"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-43.html
#[test]
fn w3c_descendant_combinator() {
    let html = r#"
 <div class="t1">
  <p class="red">This paragraph should have a green background</p>
  <table>
   <tbody>
    <tr>
     <td>
      <p class="red">This paragraph should have a green background</p>
     </td>
    </tr>
   </tbody>
  </table>
 </div>
 <table>
  <tbody>
   <tr>
    <td>
     <p class="white">This paragraph should be unstyled.</p>
    </td>
   </tr>
  </tbody>
 </table>
"#;

    assert_eq!(select(html, "div.t1 p"), ["p.red", "p.red"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-43b.html
#[test]
fn w3c_descendant_combinator_b() {
    let html = r#"
 <div class="t1">
  <p class="white">This paragraph should be unstyled</p>
  <table>
   <tbody>
    <tr>
     <td>
      <p class="white">This paragraph should be unstyled</p>
     </td>
    </tr>
   </tbody>
  </table>
 </div>
 <table>
  <tbody>
   <tr>
    <td>
     <p class="green">This paragraph should have a green background</p>
    </td>
   </tr>
  </tbody>
 </table>
"#;

    assert_eq!(select(html, "div.t1 p"), ["p.white", "p.white"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-44.html
#[test]
fn w3c_child_combinator() {
    let html = r#"
 <div>
  <p class="red test">This paragraph should have a green background</p>
  <div>
   <p class="red test">This paragraph should have a green background</p>
  </div>
 </div>
 <table>
  <tbody>
   <tr>
    <td>
     <p class="white test">This paragraph should be unstyled.</p>
    </td>
   </tr>
  </tbody>
 </table>
"#;

    assert_eq!(select(html, "div > p.test"), ["p.red.test", "p.red.test"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-44b.html
#[test]
fn w3c_child_combinator_b() {
    let html = r#"
 <div>
  <p class="white test">This paragraph should be unstyled.</p>
  <div>
   <p class="white test">This paragraph should be unstyled.</p>
  </div>
 </div>
 <table>
  <tbody>
   <tr>
    <td>
     <p class="green test">This paragraph should have a green background.</p>
    </td>
   </tr>
  </tbody>
 </table>
"#;

    assert_eq!(
        select(html, "div > p.test"),
        ["p.white.test", "p.white.test"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-44c.html
#[test]
fn w3c_child_combinator_and_classes() {
    let html = r#"
  <div> This should be unstyled. </div>
  <div class="control"> This should have a green background. </div>
"#;

    assert!(select(html, ".fail > div").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-44d.html
#[test]
fn w3c_child_combinator_and_ids() {
    let html = r#"
  <div> This should be unstyled. </div>
  <p> This should have a green background. </p>
"#;

    assert!(select(html, "#fail > div").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-45.html
#[test]
fn w3c_direct_adjacent_combinator() {
    let html = r#"
 <div class="stub">
  <p>This paragraph should be unstyled.</p>
  <p class="red">But this one should have a green background.</p>
  <p class="red">And this one should also have a green background.</p>
  <address>This address is only here to fill some space between two paragraphs.</address>
  <p>This paragraph should be unstyled.</p>
 </div>
"#;

    assert_eq!(select(html, "div.stub > p + p"), ["p.red", "p.red"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-45b.html
#[test]
fn w3c_direct_adjacent_combinator_b() {
    let html = r#"
 <div class="stub">
  <p class="green">This paragraph should have a green background.</p>
  <p class="white">But this one should be unstyled.</p>
  <p class="white">And this one should also be unstyled.</p>
  <address class="green">This address is only here to fill some space between two paragraphs and should have a green background.</address>
  <p class="green">This paragraph should have a green background too.</p>
 </div>
"#;

    assert_eq!(select(html, "div.stub > p + p"), ["p.white", "p.white"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-45c.html
#[test]
fn w3c_direct_adjacent_combinator_and_classes() {
    let html = r#"
<div> This should be unstyled. </div>
  <div class="control"> This should have a green background. </div>
"#;

    assert!(select(html, ".fail + div").is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-46.html
#[test]
fn w3c_indirect_adjacent_combinator() {
    let html = r#"
 <div class="stub">
  <p>This paragraph should be unstyled.</p>
  <p class="red">But this one should have a green background</p>
  <p class="red">And this one should also have a green background</p>
  <address>This address is only here to fill some space between two paragraphs</address>
 <p class="red">This paragraph should have a green background</p>
 </div>
"#;

    assert_eq!(
        select(html, "div.stub > p ~ p"),
        ["p.red", "p.red", "p.red"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-46b.html
#[test]
fn w3c_indirect_adjacent_combinator_b() {
    let html = r#"
 <div class="stub">
  <p>This paragraph should be unstyled.</p>
  <p class="green">But this one should have a green background</p>
  <p class="green">And this one should also have a green background</p>
  <address>This address is only here to fill some space between two paragraphs</address>
  <p class="green">This paragraph should have a green background</p>
 </div>
"#;

    assert_eq!(
        select(html, "div.stub > p ~ p"),
        ["p.green", "p.green", "p.green"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-54.html
#[test]
fn w3c_negated_substring_matching_attribute_start() {
    let html = r#"
<div class="stub">
<p class="red">This paragraph should be in green characters.</p>
<p title="on chante?" class="red">This paragraph should be in green characters.</p>
<p title="si on chantait">
     <span title="si il chantait">This paragraph should be in green characters.</span>
</p>
</div>
"#;

    assert_eq!(
        select(html, r#"div.stub :not([title^="si on"])"#),
        ["p.red", "p.red", "span"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-55.html
#[test]
fn w3c_negated_substring_matching_attribute_end() {
    let html = r#"
<div class="stub">
<p class="red">This paragraph should be in green characters.</p>
<p title="on chante?" class="red">This paragraph should be in green characters.</p>
<p title="si on chantait">
     <span title="si il chante">This paragraph should be in green characters.</span>
</p>
</div>
"#;

    assert_eq!(
        select(html, r#"div.stub :not([title$="tait"])"#),
        ["p.red", "p.red", "span"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-56.html
#[test]
fn w3c_negated_substring_matching_attribute_middle() {
    let html = r#"
<div class="stub">
<p class="red">This paragraph should be in green characters.</p>
<p title="on chante?" class="red">This paragraph should be in green characters.</p>
<p title="si on chantait">
     <span title="si il chante">This paragraph should be in green characters.</span>
</p>
</div>
"#;

    assert_eq!(
        select(html, r#"div.stub :not([title*=" on"])"#),
        ["p.red", "p.red", "span"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-59.html
#[test]
fn w3c_negated_class_selector() {
    let html = r#"
<div class="stub">
<p>This paragraph should be in green characters.</p>
<p class="bar foofoo tut">This paragraph should be in green characters.</p>
<p class="bar foo tut">
     <span class="tut foo2">This paragraph should be in green characters.</span>
</p>
</div>
"#;

    assert_eq!(
        select(html, r#"div.stub :not(.foo)"#),
        ["p", "p.bar.foofoo.tut", "span.tut.foo2"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-60.html
#[test]
fn w3c_negated_id_selector() {
    let html = r#"
<div class="stub">
<p>This paragraph should be in green characters.</p>
<p id="foo2">This paragraph should be in green characters.</p>
<p id="foo">
     <span>This paragraph should be in green characters.</span>
</p>
</div>
"#;

    assert_eq!(
        select(html, r#"div.stub :not(#foo)"#),
        ["p", "p#foo2", "span"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-72.html
#[test]
fn w3c_negated_root_pseudo_class() {
    let html = r#"
 <div>
  <p>This paragraph should have a green background and there should be no red anywhere.</p>
 </div>
"#;

    assert_eq!(select(html, r#"p:not(:root)"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-72b.html
#[test]
fn w3c_negated_root_pseudo_class_b() {
    let html = r#"
<div>
  <p>This paragraph should have a green background and there should be no red anywhere.</p>
 </div>
"#;

    // `html:not(:root)` matches nothing (no `<html>` in the fragment) and `test` is an
    // unknown element. Since ADR 0002 §4 an unknown tag name is a valid *extended* selector
    // (matching nothing here) rather than a parse error, so the list parses and selects
    // nothing — the same visual result the W3C test intends.
    assert_eq!(
        select(html, r#"html:not(:root), test:not(:root)"#),
        Vec::<String>::new()
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-73.html
#[test]
fn w3c_negated_nth_child_pseudo_class() {
    let html = r#"
<ul>
  <li>First list item</li>
  <li class="red">This second list item should have a green background</li>
  <li>Third list</li>
  <li class="red">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="red">This sixth list item should have a green background</li>
</ul>
<ol>
  <li class="red">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="red">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="red">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ol>
<div>
<table border="1" class="t1">
  <tr>
<td>1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr>
<td>2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr>
<td>3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr>
<td>4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr class="red">
<td>Green row : 5.1</td>
<td>5.2</td>
<td>5.3</td>
</tr>
  <tr class="red">
<td>Green row : 6.1</td>
<td>6.2</td>
<td>6.3</td>
</tr>
</table>
<p></p>
<table class="t2" border="1">
  <tr>
<td>1.1</td>
<td class="red">green cell</td>
<td class="red">green cell</td>
      <td>1.4</td>
<td class="red">green cell</td>
<td class="red">green cell</td>
      <td>1.7</td>
<td class="red">green cell</td>
</tr>
  <tr>
<td>2.1</td>
<td class="red">green cell</td>
<td class="red">green cell</td>
      <td>2.4</td>
<td class="red">green cell</td>
<td class="red">green cell</td>
      <td>2.7</td>
<td class="red">green cell</td>
</tr>
  <tr>
<td>3.1</td>
<td class="red">green cell</td>
<td class="red">green cell</td>
      <td>3.4</td>
<td class="red">green cell</td>
<td class="red">green cell</td>
      <td>3.7</td>
<td class="red">green cell</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, r#"ul > li:not(:nth-child(odd))"#),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, r#"ol > li:not(:nth-child(even))"#),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, r#"table.t1 tr:not(:nth-child(-n+4))"#),
        ["tr.red", "tr.red"]
    );
    assert_eq!(
        select(html, r#"table.t2 td:not(:nth-child(3n+1))"#),
        [
            "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red",
            "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-73b.html
#[test]
fn w3c_negated_nth_child_pseudo_class_b() {
    let html = r#"
<ul>
  <li>First list item</li>
  <li class="green">This second list item should have a green background</li>
  <li>Third list</li>
  <li class="green">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="green">This sixth list item should have a green background</li>
</ul>
<ol>
  <li class="green">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="green">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="green">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ol>
<div>
<table border="1" class="t1">
  <tr>
<td>1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr>
<td>2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr>
<td>3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr>
<td>4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr class="green">
<td>Green row : 5.1</td>
<td>5.2</td>
<td>5.3</td>
</tr>
  <tr class="green">
<td>Green row : 6.1</td>
<td>6.2</td>
<td>6.3</td>
</tr>
</table>
<p></p>
<table class="t2" border="1">
  <tr>
<td>1.1</td>
<td class="green">green cell</td>
<td class="green">green cell</td>
      <td>1.4</td>
<td class="green">green cell</td>
<td class="green">green cell</td>
      <td>1.7</td>
<td class="green">green cell</td>
</tr>
  <tr>
<td>2.1</td>
<td class="green">green cell</td>
<td class="green">green cell</td>
      <td>2.4</td>
<td class="green">green cell</td>
<td class="green">green cell</td>
      <td>2.7</td>
<td class="green">green cell</td>
</tr>
  <tr>
<td>3.1</td>
<td class="green">green cell</td>
<td class="green">green cell</td>
      <td>3.4</td>
<td class="green">green cell</td>
<td class="green">green cell</td>
      <td>3.7</td>
<td class="green">green cell</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, r#"ul > li:not(:nth-child(odd))"#),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, r#"ol > li:not(:nth-child(even))"#),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, r#"table.t1 tr:not(:nth-child(-n+4))"#),
        ["tr.green", "tr.green"]
    );
    assert_eq!(
        select(html, r#"table.t2 td:not(:nth-child(3n+1))"#),
        [
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
            "td.green"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-74.html
#[test]
fn w3c_negated_nth_last_child_pseudo_class() {
    let html = r#"
<ul>
  <li class="red">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="red">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="red">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ul>
<ol>
  <li>First list item</li>
  <li class="red">This second list item should have a green background</li>
  <li>Third list item</li>
  <li class="red">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="red">This sixth list item should have a green background</li>
</ol>
<div>
<table border="1" class="t1">
  <tr class="red">
<td>Green row : 1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr class="red">
<td>Green row : 2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr>
<td>3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr>
<td>4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr>
<td>5.1</td>
<td>5.2</td>
      <td>5.3</td>
</tr>
  <tr>
<td>6.1</td>
<td>6.2</td>
      <td>6.3</td>
</tr>
</table>
<p></p>
<table class="t2" border="1">
  <tr>
<td class="red">green cell</td>
<td>1.2</td>
<td class="red">green cell</td>
      <td class="red">green cell</td>
<td>1.5</td>
<td class="red">green cell</td>
      <td class="red">green cell</td>
<td>1.8</td>
</tr>
  <tr>
<td class="red">green cell</td>
<td>2.2</td>
<td class="red">green cell</td>
      <td class="red">green cell</td>
<td>2.5</td>
<td class="red">green cell</td>
      <td class="red">green cell</td>
<td>2.8</td>
</tr>
  <tr>
<td class="red">green cell</td>
<td>3.2</td>
<td class="red">green cell</td>
      <td class="red">green cell</td>
<td>3.5</td>
<td class="red">green cell</td>
      <td class="red">green cell</td>
<td>3.8</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, r#"ul > li:not(:nth-last-child(odd))"#),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, r#"ol > li:not(:nth-last-child(even))"#),
        ["li.red", "li.red", "li.red"]
    );
    assert_eq!(
        select(html, r#"table.t1 tr:not(:nth-last-child(-n+4))"#),
        ["tr.red", "tr.red"]
    );
    assert_eq!(
        select(html, r#"table.t2 td:not(:nth-last-child(3n+1))"#),
        [
            "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red",
            "td.red", "td.red", "td.red", "td.red", "td.red", "td.red", "td.red"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-74b.html
#[test]
fn w3c_negated_nth_last_child_pseudo_class_b() {
    let html = r#"
<ul>
  <li class="green">This first list item should have a green background</li>
  <li>Second list item</li>
  <li class="green">This third list item should have a green background</li>
  <li>Fourth list item</li>
  <li class="green">This fifth list item should have a green background</li>
  <li>Sixth list item</li>
</ul>
<ol>
  <li>First list item</li>
  <li class="green">This second list item should have a green background</li>
  <li>Third list item</li>
  <li class="green">This fourth list item should have a green background</li>
  <li>Fifth list item</li>
  <li class="green">This sixth list item should have a green background</li>
</ol>
<div>
<table border="1" class="t1">
  <tr class="green">
<td>Green row : 1.1</td>
<td>1.2</td>
     <td>1.3</td>
</tr>
  <tr class="green">
<td>Green row : 2.1</td>
<td>2.2</td>
     <td>2.3</td>
</tr>
  <tr>
<td>3.1</td>
<td>3.2</td>
     <td>3.3</td>
</tr>
  <tr>
<td>4.1</td>
<td>4.2</td>
      <td>4.3</td>
</tr>
  <tr>
<td>5.1</td>
<td>5.2</td>
      <td>5.3</td>
</tr>
  <tr>
<td>6.1</td>
<td>6.2</td>
      <td>6.3</td>
</tr>
</table>
<p></p>
<table class="t2" border="1">
  <tr>
<td class="green">green cell</td>
<td>1.2</td>
<td class="green">green cell</td>
      <td class="green">green cell</td>
<td>1.5</td>
<td class="green">green cell</td>
      <td class="green">green cell</td>
<td>1.8</td>
</tr>
  <tr>
<td class="green">green cell</td>
<td>2.2</td>
<td class="green">green cell</td>
      <td class="green">green cell</td>
<td>2.5</td>
<td class="green">green cell</td>
      <td class="green">green cell</td>
<td>2.8</td>
</tr>
  <tr>
<td class="green">green cell</td>
<td>3.2</td>
<td class="green">green cell</td>
      <td class="green">green cell</td>
<td>3.5</td>
<td class="green">green cell</td>
      <td class="green">green cell</td>
<td>3.8</td>
</tr>
</table>
</div>
"#;

    assert_eq!(
        select(html, r#"ul > li:not(:nth-last-child(odd))"#),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, r#"ol > li:not(:nth-last-child(even))"#),
        ["li.green", "li.green", "li.green"]
    );
    assert_eq!(
        select(html, r#"table.t1 tr:not(:nth-last-child(-n+4))"#),
        ["tr.green", "tr.green"]
    );
    assert_eq!(
        select(html, r#"table.t2 td:not(:nth-last-child(3n+1))"#),
        [
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
            "td.green"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-75.html
#[test]
fn w3c_negated_nth_of_type_pseudo_class() {
    let html = r#"
<p class="red">This paragraph should have green background</p>
<address>And this address should be unstyled.</address>
<p class="red">This paragraph should also have green background!</p>
<p>But this one should be unstyled again.</p>
<dl>
  <dt>First definition term</dt>
    <dd>First definition</dd>
  <dt class="red">Second definition term that should have green background</dt>
    <dd class="red">Second definition that should have green background</dd>
  <dt class="red">Third definition term that should have green background</dt>
    <dd class="red">Third definition that should have green background</dd>
  <dt>Fourth definition term</dt>
    <dd>Fourth definition</dd>
  <dt class="red">Fifth definition term that should have green background</dt>
    <dd class="red">Fifth definition that should have green background</dd>
  <dt class="red">Sixth definition term that should have green background</dt>
    <dd class="red">Sixth definition that should have green background</dd>
</dl>
"#;

    assert_eq!(
        select(html, r#"p:not(:nth-of-type(3))"#),
        ["p.red", "p.red",]
    );
    assert_eq!(
        select(html, r#"dl > :not(:nth-of-type(3n+1))"#),
        [
            "dt.red", "dd.red", "dt.red", "dd.red", "dt.red", "dd.red", "dt.red", "dd.red"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-75b.html
#[test]
fn w3c_negated_nth_of_type_pseudo_class_b() {
    let html = r#"
<p class="green">This paragraph should have green background</p>
<address>And this address should be unstyled.</address>
<p class="green">This paragraph should also have green background!</p>
<p>But this one should be unstyled again.</p>
<dl>
  <dt>First definition term</dt>
    <dd>First definition</dd>
  <dt class="green">Second definition term that should have green background</dt>
    <dd class="green">Second definition that should have green background</dd>
  <dt class="green">Third definition term that should have green background</dt>
    <dd class="green">Third definition that should have green background</dd>
  <dt>Fourth definition term</dt>
    <dd>Fourth definition</dd>
  <dt class="green">Fifth definition term that should have green background</dt>
    <dd class="green">Fifth definition that should have green background</dd>
  <dt class="green">Sixth definition term that should have green background</dt>
    <dd class="green">Sixth definition that should have green background</dd>
</dl>
"#;

    assert_eq!(
        select(html, r#"p:not(:nth-of-type(3))"#),
        ["p.green", "p.green",]
    );
    assert_eq!(
        select(html, r#"dl > :not(:nth-of-type(3n+1))"#),
        [
            "dt.green", "dd.green", "dt.green", "dd.green", "dt.green", "dd.green", "dt.green",
            "dd.green"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-76.html
#[test]
fn w3c_negated_nth_last_of_type_pseudo_class() {
    let html = r#"
<p>This paragraph should be unstyled.</p>
<address>This address should be unstyled.</address>
<p class="red">This paragraph should have green background.</p>
<p class="red">This paragraph should have green background.</p>
<dl>
  <dt class="red">First definition term that should have green background.</dt>
    <dd class="red">First definition that should also have a green background.</dd>
  <dt class="red">Second definition term that should have green background.</dt>
    <dd class="red">Second definition that should have green background.</dd>
  <dt>Third definition term.</dt>
    <dd>Third definition.</dd>
  <dt class="red">Fourth definition term that should have green background.</dt>
    <dd class="red">Fourth definition that should have green background.</dd>
  <dt class="red">Fifth definition term that should have green background.</dt>
    <dd class="red">Fifth definition that should have green background.</dd>
  <dt>Sixth definition term.</dt>
    <dd>Sixth definition.</dd>
</dl>
"#;

    assert_eq!(
        select(html, r#"p:not(:nth-last-of-type(3))"#),
        ["p.red", "p.red",]
    );
    assert_eq!(
        select(html, r#"dl > :not(:nth-last-of-type(3n+1))"#),
        [
            "dt.red", "dd.red", "dt.red", "dd.red", "dt.red", "dd.red", "dt.red", "dd.red"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-76b.html
#[test]
fn w3c_negated_nth_last_of_type_pseudo_class_b() {
    let html = r#"
<p>This paragraph should be unstyled.</p>
<address>This address should be unstyled.</address>
<p class="green">This paragraph should have green background.</p>
<p class="green">This paragraph should have green background.</p>
<dl>
  <dt class="green">First definition term that should have green background.</dt>
    <dd class="green">First definition that should also have a green background.</dd>
  <dt class="green">Second definition term that should have green background.</dt>
    <dd class="green">Second definition that should have green background.</dd>
  <dt>Third definition term.</dt>
    <dd>Third definition.</dd>
  <dt class="green">Fourth definition term that should have green background.</dt>
    <dd class="green">Fourth definition that should have green background.</dd>
  <dt class="green">Fifth definition term that should have green background.</dt>
    <dd class="green">Fifth definition that should have green background.</dd>
  <dt>Sixth definition term.</dt>
    <dd>Sixth definition.</dd>
</dl>
"#;

    assert_eq!(
        select(html, r#"p:not(:nth-last-of-type(3))"#),
        ["p.green", "p.green",]
    );
    assert_eq!(
        select(html, r#"dl > :not(:nth-last-of-type(3n+1))"#),
        [
            "dt.green", "dd.green", "dt.green", "dd.green", "dt.green", "dd.green", "dt.green",
            "dd.green"
        ]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-77.html
#[test]
fn w3c_negated_first_child_pseudo_class() {
    let html = r#"
 <div>
  <table class="t1" border="1">
   <tr>
    <td>1.1</td>
    <td class="red">green cell</td>
    <td class="red">green cell</td>
   </tr>
   <tr>
    <td>2.1</td>
    <td class="red">green cell</td>
    <td class="red">green cell</td>
   </tr>
   <tr>
    <td>3.1</td>
    <td class="red">green cell</td>
    <td class="red">green cell</td>
   </tr>
 </table>
 </div>
 <p>This paragraph <span>should be</span> unstyled.</p>
"#;

    assert_eq!(
        select(html, r#".t1 td:not(:first-child)"#),
        ["td.red", "td.red", "td.red", "td.red", "td.red", "td.red",]
    );
    assert!(select(html, r#"p > :not(:first-child)"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-77b.html
#[test]
fn w3c_negated_first_child_pseudo_class_b() {
    let html = r#"
 <div>
  <table class="t1" border="1">
   <tr>
    <td>1.1</td>
    <td class="green">green cell</td>
    <td class="green">green cell</td>
   </tr>
   <tr>
    <td>2.1</td>
    <td class="green">green cell</td>
    <td class="green">green cell</td>
   </tr>
   <tr>
    <td>3.1</td>
    <td class="green">green cell</td>
    <td class="green">green cell</td>
   </tr>
 </table>
 </div>
 <p>This paragraph <span>should be</span> unstyled.</p>
"#;

    assert_eq!(
        select(html, r#".t1 td:not(:first-child)"#),
        [
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
        ]
    );
    assert!(select(html, r#"p > :not(:first-child)"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-78.html
#[test]
fn w3c_negated_last_child_pseudo_class() {
    let html = r#"
 <div>
  <table class="t1" border="1">
   <tr>
    <td class="red">green cell</td>
    <td class="red">green cell</td>
    <td>1.3</td>
   </tr>
   <tr>
    <td class="red">green cell</td>
    <td class="red">green cell</td>
    <td>2.3</td>
   </tr>
   <tr>
    <td class="red">green cell</td>
    <td class="red">green cell</td>
    <td>3.3</td>
   </tr>
  </table>
 </div>
 <p>This <span>paragraph should</span> be unstyled.</p>
"#;

    assert_eq!(
        select(html, r#".t1 td:not(:last-child)"#),
        ["td.red", "td.red", "td.red", "td.red", "td.red", "td.red",]
    );
    assert!(select(html, r#"p > :not(:last-child)"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-78b.html
#[test]
fn w3c_negated_last_child_pseudo_class_b() {
    let html = r#"
 <div>
  <table class="t1" border="1">
   <tr>
    <td class="green">green cell</td>
    <td class="green">green cell</td>
    <td>1.3</td>
   </tr>
   <tr>
    <td class="green">green cell</td>
    <td class="green">green cell</td>
    <td>2.3</td>
   </tr>
   <tr>
    <td class="green">green cell</td>
    <td class="green">green cell</td>
    <td>3.3</td>
   </tr>
  </table>
 </div>
 <p>This <span>paragraph should</span> be unstyled.</p>
"#;

    assert_eq!(
        select(html, r#".t1 td:not(:last-child)"#),
        [
            "td.green", "td.green", "td.green", "td.green", "td.green", "td.green",
        ]
    );
    assert!(select(html, r#"p > :not(:last-child)"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-79.html
#[test]
fn w3c_negated_first_of_type_pseudo_class() {
    let html = r#"
<div>This div contains 3 addresses :
<address>A first address with normal background</address>
<address class="red">A second address that should have a green background</address>
<address class="red">A third address that should have a green background</address>
</div>
"#;

    assert_eq!(
        select(html, r#"address:not(:first-of-type)"#),
        ["address.red", "address.red"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-80.html
#[test]
fn w3c_negated_last_of_type_pseudo_class() {
    let html = r#"
<div>
<address class="red">A first address that should have a green background</address>
<address class="red">A second address that should have a green background</address>
<address>A third address with normal background</address>
This div should have three addresses above it.</div>
"#;

    assert_eq!(
        select(html, r#"address:not(:last-of-type)"#),
        ["address.red", "address.red"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-81.html
#[test]
fn w3c_negated_only_child_pseudo_class() {
    let html = r#"
 <p class="red">This paragraph should have a green background.</p>
 <div>This div contains only one paragraph.
  <p>This paragraph should be unstyled.</p>
 </div>
"#;

    assert_eq!(select(html, r#"p:not(:only-child)"#), ["p.red"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-81b.html
#[test]
fn w3c_negated_only_child_pseudo_class_b() {
    let html = r#"
<p class="green">This paragraph should have a green background.</p>
 <div>This div contains only one paragraph.
  <p>This paragraph should be unstyled.</p>
 </div>
"#;

    assert_eq!(select(html, r#"p:not(:only-child)"#), ["p.green"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-82.html
#[test]
fn w3c_negated_only_of_type_pseudo_class() {
    let html = r#"
<div class="t1">
<p class="red">This paragraph should have green background.</p>
<address>But this address should be unstyled.</address>
<p class="red">This paragraph should have green background.</p>
</div>
"#;

    assert_eq!(
        select(html, r#".t1 :not(:only-of-type)"#),
        ["p.red", "p.red"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-82b.html
#[test]
fn w3c_negated_only_of_type_pseudo_class_b() {
    let html = r#"
<div class="t1">
<p class="green">This paragraph should have green background.</p>
<address>But this address should be unstyled.</address>
<p class="green">This paragraph should have green background.</p>
</div>
"#;

    assert_eq!(
        select(html, r#".t1 :not(:only-of-type)"#),
        ["p.green", "p.green"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-83.html
#[test]
fn w3c_negation_pseudo_class_cannot_be_an_argument_of_itself() {
    let html = r#"
<p>This paragraph should have a green background</p>
"#;

    assert_eq!(select(html, r#"p:not(:not(p))"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-86.html
#[test]
fn w3c_nondeterministic_descendant_and_child_combinator() {
    let html = r#"
<blockquote>
<div>
<div>
<p>This text should be green.</p>
</div>
</div>
</blockquote>
"#;

    assert_eq!(select(html, r#"blockquote > div p"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-87.html
#[test]
fn w3c_nondeterministic_direct_and_indirect_adjacent_combinator() {
    let html = r#"
<blockquote><div>This text should be unstyled.</div></blockquote>
<div>This text should be unstyled.</div>
<div>This text should be unstyled.</div>
<p>This text should be green.</p>
"#;

    assert_eq!(select(html, r#"blockquote + div ~ p"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-88.html
#[test]
fn w3c_nondeterministic_descendant_and_direct_adjacent_combinator() {
    let html = r#"
<blockquote><div>This text should be unstyled.</div></blockquote>
<div>
<div>
<p>This text should be green.</p>
</div>
</div>
"#;

    assert_eq!(select(html, r#"blockquote + div p"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-89.html
#[test]
fn w3c_combination_of_descendant_and_child_combinator() {
    let html = r#"
<blockquote>
<div>
<div>
<p>This text should be green.</p>
</div>
</div>
</blockquote>
"#;

    assert_eq!(select(html, r#"blockquote div > p"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-90.html
#[test]
fn w3c_combination_of_direct_and_indirect_adjacent_combinator() {
    let html = r#"
<blockquote><div>This text should be unstyled.</div></blockquote>
<div>This text should be unstyled.</div>
<div>This text should be unstyled.</div>
<p>This text should be green.</p>
"#;

    assert_eq!(select(html, r#"blockquote ~ div + p"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-148.html
#[test]
fn w3c_empty_pseudo_class_and_text() {
    let html = r#"
 <p>This line should have a green background.</p>
"#;

    assert_eq!(select(html, r#"p:empty"#), ["p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-149.html
#[test]
fn w3c_empty_pseudo_class_and_empty_elements() {
    let html = r#"
 <address></address>
 <div class="text">This line should have a green background.</div>
"#;

    assert_eq!(select(html, r#"address:empty"#), ["address"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-151.html
#[test]
fn w3c_empty_pseudo_class_and_whitespace() {
    let html = r#"
 <address> </address>
 <div class="text">This line should have a green background.</div>
"#;

    assert_eq!(select(html, r#"address:empty"#), ["address"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-152.html
#[test]
fn w3c_empty_pseudo_class_and_elements() {
    let html = r#"
 <address><span></span></address>
 <div class="text">This line should have a green background.</div>
"#;

    assert!(select(html, r#"address:empty"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-170.html
#[test]
fn w3c_long_chains_of_selectors() {
    let html = r#"
   <p><span>This line should be green.</span></p>
"#;

    assert_eq!(
        select(
            html,
            r#"  span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span, span
"#
        ),
        ["span"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-170a.html
#[test]
fn w3c_long_chains_of_selectors_a() {
    let html = r#"
   <p class="span">This line should be green.</p>
"#;

    assert_eq!(
        select(
            html,
            r#"  .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span, .span

"#
        ),
        ["p.span"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-170b.html
#[test]
fn w3c_long_chains_of_selectors_b() {
    let html = r#"
   <p class="span">This line should be green.</p>
"#;

    assert_eq!(
        select(
            html,
            r#"  .span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span.span
"#
        ),
        ["p.span"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-170c.html
#[test]
fn w3c_long_chains_of_selectors_c() {
    let html = r#"
   <p>This line should be green.</p>
"#;

    assert_eq!(
        select(
            html,
            r#"  p:not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span):not(.span)
"#
        ),
        ["p"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-170d.html
#[test]
fn w3c_long_chains_of_selectors_d() {
    let html = r#"
   <p>This line should be green.</p>
"#;

    assert_eq!(
        select(
            html,
            r#"  p:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child:first-child
"#
        ),
        ["p"]
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-176.html
#[test]
fn w3c_classes_and_ids() {
    let html = r#"
    <p id="id" class="class test">This line should be green.</p>
  <div id="theid" class="class test">This line should be green.</div>
"#;

    assert_eq!(
        select(html, r#"p:not(#other).class:not(.fail).test#id#id"#),
        ["p#id.class.test"]
    );
    assert!(select(html, r#"div:not(#theid).class:not(.fail).test#theid#theid"#).is_empty());
    assert!(
        select(
            html,
            r#"div:not(#other).notclass:not(.fail).test#theid#theid"#
        )
        .is_empty()
    );
    assert!(select(html, r#"div:not(#other).class:not(.test).test#theid#theid"#).is_empty());
    assert!(
        select(
            html,
            r#"div:not(#other).class:not(.fail).nottest#theid#theid"#
        )
        .is_empty()
    );
    assert!(
        try_select(
            html,
            r#"div:not(#other).class:not(.fail).nottest#theid#other"#
        )
        .is_err()
    );
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-184a.html
#[ignore = "Ignored due to custom attribute selection for classes"]
#[test]
fn w3c_ends_with_attribute_selector_with_empty_value() {
    let html = r#"<p class="">This text should be green.</p>
<p>This text should be green.</p>
"#;

    assert!(select(html, r#"p[class$=""]"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-184b.html
#[ignore = "Ignored due to custom attribute selection for classes"]
#[test]
fn w3c_starts_with_attribute_selector_with_empty_value() {
    let html = r#"
    <p class="">This text should be green.</p>
<p>This text should be green.</p>
"#;

    assert!(select(html, r#"p[class^=""]"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-184c.html
#[ignore = "Ignored due to custom attribute selection for classes"]
#[test]
fn w3c_contains_attribute_selector_with_empty_value() {
    let html = r#"
    <p class="">This text should be green.</p>
<p>This text should be green.</p>
"#;

    assert!(select(html, r#"p[class*=""]"#).is_empty());
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-184d.html
#[ignore = "Ignored due to custom attribute selection for classes"]
#[test]
fn w3c_negated_ends_with_attribute_selector_with_empty_value() {
    let html = r#"
    <p class="">This text should be green.</p>
<p>This text should be green.</p>
"#;

    assert_eq!(select(html, r#"p:not([class$=""])"#), ["p", "p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-184e.html
#[ignore = "Ignored due to custom attribute selection for classes"]
#[test]
fn w3c_negated_starts_with_attribute_selector_with_empty_value() {
    let html = r#"
    <p class="">This text should be green.</p>
<p>This text should be green.</p>
"#;

    assert_eq!(select(html, r#"p:not([class^=""])"#), ["p", "p"]);
}

/// https://www.w3.org/Style/CSS/Test/CSS3/Selectors/current/html/full/flat/css3-modsel-184f.html
#[ignore = "Ignored due to custom attribute selection for classes"]
#[test]
fn w3c_negated_contains_attribute_selector_with_empty_value() {
    let html = r#"
    <p class="">This text should be green.</p>
<p>This text should be green.</p>
"#;

    assert_eq!(select(html, r#"p:not([class*=""])"#), ["p", "p"]);
}
