use fs_err as fs;
use std::collections::HashSet;

use htmlarc_dom::{css::*, prelude::*};

/// An include/exclude predicate over archive entries, built from CSS-selector and
/// word-list rules.
///
/// Each rule string is one of `css:<selector>`, `words:<comma,separated,list>`, or a
/// path to a `.tsv` word file. An entry is kept when it matches the include rules
/// (or there are none) and matches none of the exclude rules. Use it directly via
/// [`keep`](Self::keep), or apply it across an archive with
/// [`Archive::entries_matching`](crate::Archive::entries_matching).
pub struct Filter {
    include: WordFilter,
    exclude: WordFilter,
}

impl Filter {
    pub fn new(include: Vec<String>, exclude: Vec<String>) -> Result<Self, FilterError> {
        let include = WordFilter::new(include)?;
        let exclude = WordFilter::new(exclude)?;
        Ok(Self { include, exclude })
    }

    pub fn keep<Dom: DomRead>(&self, word: &str, dom: &Dom) -> bool {
        let included = if self.include.is_empty() {
            true
        } else {
            self.include.matches(word, dom)
        };
        let excluded = if self.exclude.is_empty() {
            false
        } else {
            self.exclude.matches(word, dom)
        };

        included && !excluded
    }

    /// The set of keys the *include* filter restricts to, if any.
    ///
    /// The include predicate ANDs key-membership with its CSS rules, so when a `words:`/`.tsv`
    /// rule is present **every kept entry's key is in this set**. A caller can then resolve just
    /// these keys through the archive's keyed index instead of scanning every document. `None`
    /// means the include filter places no constraint on the key (no `words:`/`.tsv` rule), so a
    /// full scan is required.
    pub fn include_keys(&self) -> Option<&HashSet<String>> {
        if self.include.words.is_empty() {
            None
        } else {
            Some(&self.include.words)
        }
    }

    /// Decide the filter from the key alone, when no CSS rule is involved (so the document body
    /// is irrelevant). Returns `None` if any CSS rule means the DOM must be inspected — the caller
    /// must then materialize the document and call [`keep`](Self::keep). Lets a keyed fast path
    /// avoid touching document blobs entirely for pure word/key filters.
    pub fn keep_key(&self, key: &str) -> Option<bool> {
        if !self.include.css.is_empty() || !self.exclude.css.is_empty() {
            return None;
        }
        let included = self.include.words.is_empty() || self.include.words.contains(key);
        let excluded = !self.exclude.words.is_empty() && self.exclude.words.contains(key);
        Some(included && !excluded)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("Failed to parse filter: {0}")]
    Parse(String),
    #[error("Failed to parse css selector '{0}' : {1}")]
    Css(&'static str, String),
    #[error("Filter kind unrecognized: {0}")]
    Kind(String),
}

struct WordFilter {
    css: Vec<SelectorList<'static>>,
    words: HashSet<String>,
}

impl WordFilter {
    pub fn new(defs: Vec<String>) -> Result<Self, FilterError> {
        let mut css = Vec::new();
        let mut words: HashSet<String> = HashSet::new();

        for def in &defs {
            match def.split_once(':') {
                Some(("css", value)) => {
                    let value: &str = Box::leak(Box::new(value.to_owned()));
                    let selector =
                        parse_css(value).map_err(|e| FilterError::Css(value, e.to_string()))?;
                    css.push(selector);
                }
                Some(("words", value)) => {
                    let new_words = value.split(',').map(|w| w.trim()).map(|s| s.to_owned());

                    words.extend(new_words);
                }
                None if def.ends_with(".tsv") => {
                    let content = match fs::read_to_string(def) {
                        Ok(data) => data,
                        Err(e) => panic!("Failed to read tsv file: {e}"),
                    };
                    words.extend(content.lines().map(|l| l.to_owned()));
                }
                _ => return Err(FilterError::Kind(def.to_owned())),
            }
        }

        Ok(Self { css, words })
    }

    pub fn matches<Dom: DomRead>(&self, word: &str, dom: &Dom) -> bool {
        let match_css = if self.css.is_empty() {
            true
        } else {
            self.css.iter().all(|selector| {
                let el = dom.root();

                let mut matches = el.select(selector.clone());

                matches.next().is_some()
            })
        };

        let match_words = if self.words.is_empty() {
            true
        } else {
            self.words.contains(word)
        };

        match_css && match_words
    }
    fn is_empty(&self) -> bool {
        self.css.is_empty() && self.words.is_empty()
    }

    #[cfg(test)]
    fn string(&self) -> String {
        let css = self
            .css
            .iter()
            .map(|selector| selector.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut words = self.words.iter().map(|w| w.to_owned()).collect::<Vec<_>>();
        words.sort();

        format!("css:{css};words:{}", words.join(","))
    }
}

#[test]
fn word_filters_from_vec() {
    let vec = vec![
        "css:div".to_string(),
        "words:hello,banana".to_string(),
        "words:hello,world".to_string(),
        "css:span > i".to_string(),
    ];
    let word_filters = WordFilter::new(vec).unwrap();

    assert_eq!(
        word_filters.string(),
        "css:div,span > i;words:banana,hello,world"
    );
}

#[test]
fn include_keys_exposes_word_set_only() {
    // A words include bounds the candidate keys; a css-only / empty include does not.
    let words = Filter::new(vec!["words:a,b".to_string()], vec![]).unwrap();
    let keys = words.include_keys().expect("words include bounds the keys");
    assert_eq!(keys.len(), 2);
    assert!(keys.contains("a") && keys.contains("b"));

    assert!(
        Filter::new(vec!["css:div".to_string()], vec![])
            .unwrap()
            .include_keys()
            .is_none()
    );
    assert!(
        Filter::new(vec![], vec![])
            .unwrap()
            .include_keys()
            .is_none()
    );
}

#[test]
fn keep_key_decides_pure_word_filters_without_a_dom() {
    // include words minus exclude words is decidable from the key alone.
    let f = Filter::new(vec!["words:a,b,c".to_string()], vec!["words:b".to_string()]).unwrap();
    assert_eq!(f.keep_key("a"), Some(true));
    assert_eq!(f.keep_key("b"), Some(false)); // excluded
    assert_eq!(f.keep_key("z"), Some(false)); // not in the include set
}

#[test]
fn keep_key_defers_to_the_dom_when_css_is_involved() {
    let inc_css = Filter::new(vec!["css:div".to_string(), "words:a".to_string()], vec![]).unwrap();
    assert_eq!(inc_css.keep_key("a"), None); // include css -> must inspect the document
    let exc_css = Filter::new(vec!["words:a".to_string()], vec!["css:span".to_string()]).unwrap();
    assert_eq!(exc_css.keep_key("a"), None); // exclude css -> must inspect the document
}
