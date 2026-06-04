use std::fmt::Display;

use crate::probe::format::*;

#[derive(Debug, Clone)]
pub struct AttributeSelector<'s> {
    pub name: AttributeName<'s>,
    pub value: Option<(AttributeOperator, &'s str)>,
}

impl Display for AttributeSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}", self.name)?;

        if let Some((operator, value)) = &self.value {
            write!(f, "{}{}", operator, value)?;
        }

        write!(f, "]")
    }
}

impl<'s> AttributeSelector<'s> {
    pub fn from_chars(chars: &mut CssChars<'s>) -> Result<Option<Self>, String> {
        chars.skip_spaces();
        let Some((_, c)) = chars.current() else {
            return Ok(None);
        };

        if c != '[' {
            return Ok(None);
        }
        chars.next();

        let pattern = TextPattern::default()
            .allow_alphabetic()
            .allow_numeric()
            .start_with(CssChar::Alphabetic)
            .allow_special('-')
            .allow_special('_')
            .allow_special(':')
            .allow_special('.')
            .not_exclusively(CssChar::Digit)
            .not_exclusively(CssChar::Special('-'))
            .not_exclusively(CssChar::Special('_'))
            .not_exclusively(CssChar::Special(':'))
            .not_exclusively(CssChar::Special('.'))
            .stop_at(']');

        if let Some(name) = pattern.validate(chars).map_err(|e| e.to_string())? {
            match AttributeName::try_from(name) {
                Ok(attr) => {
                    if let Some(operator) =
                        AttributeOperator::from_chars(chars).map_err(|e| e.to_string())?
                    {
                        let text_pattern = TextPattern::default()
                            .allow_alphabetic()
                            .allow_numeric()
                            .start_with(CssChar::Alphabetic)
                            .start_with(CssChar::Digit)
                            .stop_at(']');

                        if let Some(text) =
                            text_pattern.validate(chars).map_err(|e| e.to_string())?
                        {
                            chars.next();
                            Ok(Some(Self {
                                name: attr,
                                value: Some((operator, text)),
                            }))
                        } else {
                            Err("no value".to_string())
                        }
                    } else {
                        chars.next();
                        Ok(Some(Self {
                            name: attr,
                            value: None,
                        }))
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            Ok(None)
        }
    }
}

#[test]
fn parse_attribute_selector() {
    let str = "[href]";
    let mut chars = CssChars::new(str);
    let selector = AttributeSelector::from_chars(&mut chars).unwrap().unwrap();

    assert_eq!(selector.to_string(), str);

    let str = "[class*=mw]";
    let mut chars = CssChars::new(str);
    let selector = AttributeSelector::from_chars(&mut chars).unwrap().unwrap();

    assert_eq!(selector.to_string(), str);

    let str = "[text$=foo]";
    let mut chars = CssChars::new(str);
    let selector = AttributeSelector::from_chars(&mut chars).unwrap().unwrap();

    assert_eq!(selector.to_string(), str);
}
