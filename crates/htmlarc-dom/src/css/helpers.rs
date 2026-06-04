use crate::css::{chars::CssChars, ext::OptionExt, logging::debug};

use super::{ParseError, patterns::CssPattern};

pub fn test_ok<'s, P: CssPattern<'s>>(string: &'s str, expected: Option<P>) {
    debug!("\nTesting ok: '{}'", string);
    let mut chars = CssChars::new(string);
    let result = match P::from_chars(&mut chars) {
        Ok(result) => result,
        Err(err) => {
            debug!("Error: {}", err);
            panic!();
        }
    };

    assert_eq!(result.string(), expected.string());
}

pub fn test_err<'s, P: CssPattern<'s>>(string: &'s str, expected: ParseError) {
    debug!("\nTesting err: '{}'", string);
    let mut chars = CssChars::new(string);
    let result = match P::from_chars(&mut chars) {
        Ok(result) => {
            debug!("Found: {}", result.string());
            debug!("Expected error: {}", expected);
            panic!();
        }
        Err(e) => e,
    };
    debug!("Error: {}", result);
    assert_eq!(result.to_string(), expected.to_string());
}
