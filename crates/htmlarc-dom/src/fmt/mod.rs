mod fmt_buf;
mod format;
mod iter;
mod meta_charset;
mod spaces;
#[cfg(test)]
mod tests;

pub use format::HtmlFormat;
pub(crate) use spaces::Spaces;
