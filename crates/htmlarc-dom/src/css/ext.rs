use std::fmt::Display;

pub trait OptionExt<T> {
    fn string(&self) -> String;
}

impl<T: Display> OptionExt<T> for Option<T> {
    fn string(&self) -> String {
        match self {
            Some(value) => value.to_string(),
            None => "None".to_string(),
        }
    }
}
