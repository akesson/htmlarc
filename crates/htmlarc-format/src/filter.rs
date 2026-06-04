use fs_err as fs;
use std::collections::HashSet;

use htmlarc_dom::{css::*, prelude::*};

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

    pub fn keep(&self, word: &str, dom: &DomInner) -> bool {
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

    pub fn matches(&self, word: &str, dom: &DomInner) -> bool {
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
