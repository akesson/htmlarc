#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementStyle {
    /// a standard html tag format: <h1 id='myid' class='class1 class2' title='Verb'>
    HtmlFmt,
    /// css format: h1#myid.class1.class2[title='Verb']'
    CssFmt,
    /// as CssFmt but no attribute names: h1#myid.class1.class2['Verb']
    CssTerse,
}
