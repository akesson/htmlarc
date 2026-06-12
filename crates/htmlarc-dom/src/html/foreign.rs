//! WHATWG foreign-content case restoration (ADR 0002 §5).
//!
//! html5gum lowercases every tag and attribute name before we see it, so an SVG `viewBox`
//! is stored as `viewbox` and a `clipPath` element as `clippath`. The WHATWG parser fixes
//! the case of known SVG/MathML names at tree-construction time ("adjust SVG tag names",
//! "adjust SVG attributes", and MathML's lone `definitionurl` → `definitionURL`); we instead
//! store the lowercased names verbatim — keeping selectors and the symbol table case-stable —
//! and restore the canonical spelling here, **at the formatter**.
//!
//! The restoration is context-free (keyed only on the lowercased name), matching the spec's
//! own name→name tables: a stray `<clippath>` outside any `<svg>` also renders `clipPath`.
//! Both tables are sorted by their lowercase key for `binary_search` (asserted in tests); a
//! name absent from the table is returned unchanged, so ordinary HTML and `data-*` names pay
//! only one miss.
//!
//! Source: <https://html.spec.whatwg.org/multipage/parsing.html#parsing-main-inforeign>.

/// "Adjust SVG tag names" — the lowercased element name → its canonical mixed-case spelling.
/// MathML has no tag-name adjustments (every MathML element is already lowercase).
static SVG_TAG_NAMES: &[(&str, &str)] = &[
    ("altglyph", "altGlyph"),
    ("altglyphdef", "altGlyphDef"),
    ("altglyphitem", "altGlyphItem"),
    ("animatecolor", "animateColor"),
    ("animatemotion", "animateMotion"),
    ("animatetransform", "animateTransform"),
    ("clippath", "clipPath"),
    ("feblend", "feBlend"),
    ("fecolormatrix", "feColorMatrix"),
    ("fecomponenttransfer", "feComponentTransfer"),
    ("fecomposite", "feComposite"),
    ("feconvolvematrix", "feConvolveMatrix"),
    ("fediffuselighting", "feDiffuseLighting"),
    ("fedisplacementmap", "feDisplacementMap"),
    ("fedistantlight", "feDistantLight"),
    ("fedropshadow", "feDropShadow"),
    ("feflood", "feFlood"),
    ("fefunca", "feFuncA"),
    ("fefuncb", "feFuncB"),
    ("fefuncg", "feFuncG"),
    ("fefuncr", "feFuncR"),
    ("fegaussianblur", "feGaussianBlur"),
    ("feimage", "feImage"),
    ("femerge", "feMerge"),
    ("femergenode", "feMergeNode"),
    ("femorphology", "feMorphology"),
    ("feoffset", "feOffset"),
    ("fepointlight", "fePointLight"),
    ("fespecularlighting", "feSpecularLighting"),
    ("fespotlight", "feSpotLight"),
    ("fetile", "feTile"),
    ("feturbulence", "feTurbulence"),
    ("foreignobject", "foreignObject"),
    ("glyphref", "glyphRef"),
    ("lineargradient", "linearGradient"),
    ("radialgradient", "radialGradient"),
    ("textpath", "textPath"),
];

/// "Adjust SVG attributes" plus MathML's lone `definitionurl`: lowercased attribute name →
/// its canonical mixed-case spelling. The "adjust foreign attributes" table (`xlink:href`,
/// `xml:lang`, …) needs no entries — those serialize lowercase already.
static SVG_ATTR_NAMES: &[(&str, &str)] = &[
    ("attributename", "attributeName"),
    ("attributetype", "attributeType"),
    ("basefrequency", "baseFrequency"),
    ("baseprofile", "baseProfile"),
    ("calcmode", "calcMode"),
    ("clippathunits", "clipPathUnits"),
    ("definitionurl", "definitionURL"), // MathML
    ("diffuseconstant", "diffuseConstant"),
    ("edgemode", "edgeMode"),
    ("filterunits", "filterUnits"),
    ("glyphref", "glyphRef"),
    ("gradienttransform", "gradientTransform"),
    ("gradientunits", "gradientUnits"),
    ("kernelmatrix", "kernelMatrix"),
    ("kernelunitlength", "kernelUnitLength"),
    ("keypoints", "keyPoints"),
    ("keysplines", "keySplines"),
    ("keytimes", "keyTimes"),
    ("lengthadjust", "lengthAdjust"),
    ("limitingconeangle", "limitingConeAngle"),
    ("markerheight", "markerHeight"),
    ("markerunits", "markerUnits"),
    ("markerwidth", "markerWidth"),
    ("maskcontentunits", "maskContentUnits"),
    ("maskunits", "maskUnits"),
    ("numoctaves", "numOctaves"),
    ("pathlength", "pathLength"),
    ("patterncontentunits", "patternContentUnits"),
    ("patterntransform", "patternTransform"),
    ("patternunits", "patternUnits"),
    ("pointsatx", "pointsAtX"),
    ("pointsaty", "pointsAtY"),
    ("pointsatz", "pointsAtZ"),
    ("preservealpha", "preserveAlpha"),
    ("preserveaspectratio", "preserveAspectRatio"),
    ("primitiveunits", "primitiveUnits"),
    ("refx", "refX"),
    ("refy", "refY"),
    ("repeatcount", "repeatCount"),
    ("repeatdur", "repeatDur"),
    ("requiredextensions", "requiredExtensions"),
    ("requiredfeatures", "requiredFeatures"),
    ("specularconstant", "specularConstant"),
    ("specularexponent", "specularExponent"),
    ("spreadmethod", "spreadMethod"),
    ("startoffset", "startOffset"),
    ("stddeviation", "stdDeviation"),
    ("stitchtiles", "stitchTiles"),
    ("surfacescale", "surfaceScale"),
    ("systemlanguage", "systemLanguage"),
    ("tablevalues", "tableValues"),
    ("targetx", "targetX"),
    ("targety", "targetY"),
    ("textlength", "textLength"),
    ("viewbox", "viewBox"),
    ("viewtarget", "viewTarget"),
    ("xchannelselector", "xChannelSelector"),
    ("ychannelselector", "yChannelSelector"),
    ("zoomandpan", "zoomAndPan"),
];

/// Look up `name` (already lowercased by the tokenizer) in a sorted name table.
fn adjust<'a>(table: &'static [(&'static str, &'static str)], name: &'a str) -> &'a str {
    match table.binary_search_by(|(key, _)| (*key).cmp(name)) {
        Ok(i) => table[i].1,
        Err(_) => name,
    }
}

/// Restore the canonical case of a known SVG element name; any other name is returned
/// unchanged.
pub(crate) fn adjust_tag_name(name: &str) -> &str {
    adjust(SVG_TAG_NAMES, name)
}

/// Restore the canonical case of a known SVG/MathML attribute name; any other name (a
/// `data-*` key, a plain custom attribute) is returned unchanged.
pub(crate) fn adjust_attr_name(name: &str) -> &str {
    adjust(SVG_ATTR_NAMES, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `binary_search` requires the tables sorted by key, and the key must be the canonical
    /// value lowercased (the form the tokenizer produces).
    fn assert_table_well_formed(table: &[(&str, &str)]) {
        for win in table.windows(2) {
            assert!(
                win[0].0 < win[1].0,
                "unsorted: {:?} >= {:?}",
                win[0],
                win[1]
            );
        }
        for (key, value) in table {
            assert_eq!(
                *key,
                value.to_ascii_lowercase(),
                "key must be the lowercased canonical name"
            );
        }
    }

    #[test]
    fn tables_are_sorted_and_keyed_by_lowercase() {
        assert_table_well_formed(SVG_TAG_NAMES);
        assert_table_well_formed(SVG_ATTR_NAMES);
    }

    #[test]
    fn known_names_restore_case() {
        assert_eq!(adjust_tag_name("clippath"), "clipPath");
        assert_eq!(adjust_tag_name("foreignobject"), "foreignObject");
        assert_eq!(adjust_tag_name("fegaussianblur"), "feGaussianBlur");
        assert_eq!(adjust_attr_name("viewbox"), "viewBox");
        assert_eq!(
            adjust_attr_name("preserveaspectratio"),
            "preserveAspectRatio"
        );
        assert_eq!(adjust_attr_name("definitionurl"), "definitionURL");
    }

    #[test]
    fn unknown_names_pass_through() {
        assert_eq!(adjust_tag_name("svg"), "svg");
        assert_eq!(adjust_tag_name("my-widget"), "my-widget");
        assert_eq!(adjust_attr_name("data-mw"), "data-mw");
        assert_eq!(adjust_attr_name("class"), "class");
        assert_eq!(adjust_attr_name("d"), "d");
    }
}
