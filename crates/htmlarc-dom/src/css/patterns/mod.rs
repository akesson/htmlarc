mod attribute_operator;
mod attribute_pattern;
mod attribute_value;
mod case_indicator;
mod combinator;
mod integer_pattern;
mod ordinal_pattern;
mod parenthesized;
mod quoted_string;
mod text_pattern;

pub use attribute_operator::AttributeOperator;
pub use attribute_pattern::{AttributeName, AttributePattern, AttributePatternError};
pub use attribute_value::AttributeValue;
pub use case_indicator::CaseIndicator;
pub use combinator::Combinator;
pub use ordinal_pattern::{OrdinalPattern, OrdinalPatternError};
pub use parenthesized::{Brackets, Parentheses, Parenthesized, ParenthesizedError};
pub use quoted_string::QuotedString;
pub use text_pattern::{CssChar, TextPattern, TextPatternError};

use std::fmt::{Debug, Display};

use super::{ParseResult, chars::CssChars};

pub trait CssPattern<'s>: Sized + Display + Debug {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>>;
}
