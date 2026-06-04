//! Tiny number→superscript helpers used by the probe tree output.
//! (Folded in from the original `utils::num` module.)
#![allow(dead_code)] // a complete little NumStrings API; the probe only uses a subset.

pub trait NumStrings {
    /// Return the number as a superscript string, e.g. 23 -> "²³".
    fn to_superscript(self) -> String;
    /// Return the number as a subscript string, e.g. 23 -> "₂₃".
    fn to_subscript(self) -> String;
    /// Return the number base-16 encoded as characters in the range 'ᵃ'..='ᵖ'.
    fn to_superscript_chars(self) -> String;
}

macro_rules! impl_num_strings {
    ($($t:ty),*) => {$(
        impl NumStrings for $t {
            fn to_superscript(self) -> String {
                usize_to_superscript(self as usize)
            }
            fn to_subscript(self) -> String {
                usize_to_subscript(self as usize)
            }
            fn to_superscript_chars(self) -> String {
                to_superscript_chars(self as usize)
            }
        }
    )*};
}

impl_num_strings!(usize, u64, u32, u16, u8);

fn usize_to_superscript(mut num: usize) -> String {
    let mut chars: Vec<char> = Vec::new();
    if num == 0 {
        return '⁰'.to_string();
    }
    while num > 0 {
        let rem = num % 10;
        num /= 10;
        chars.push(
            ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'][rem],
        );
    }
    chars.reverse();
    chars.into_iter().collect()
}

fn usize_to_subscript(mut num: usize) -> String {
    let mut chars: Vec<char> = Vec::new();
    if num == 0 {
        return '₀'.to_string();
    }
    while num > 0 {
        let rem = num % 10;
        num /= 10;
        chars.push(
            ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'][rem],
        );
    }
    chars.reverse();
    chars.into_iter().collect()
}

fn to_superscript_chars(mut num: usize) -> String {
    const ALPHABET: [char; 16] = [
        'ᵃ', 'ᵇ', 'ᶜ', 'ᵈ', 'ᵉ', 'ᶠ', 'ᵍ', 'ʰ', 'ⁱ', 'ʲ', 'ᵏ', 'ˡ', 'ᵐ', 'ⁿ', 'ᵒ', 'ᵖ',
    ];
    let mut chars: Vec<char> = Vec::new();
    if num == 0 {
        return ALPHABET[0].to_string();
    }
    while num > 0 {
        let rem = num % 16;
        num /= 16;
        chars.push(ALPHABET[rem]);
    }
    chars.reverse();
    chars.into_iter().collect()
}

#[test]
fn test() {
    assert_eq!("₀", 0_u32.to_subscript());
    assert_eq!("²³", 23_usize.to_superscript());
    assert_eq!("₂₃", 23_u16.to_subscript());
    assert_eq!("ᵈʰ", (55_usize).to_superscript_chars());
    assert_eq!("ᵃ", (0_u8).to_superscript_chars());
}
