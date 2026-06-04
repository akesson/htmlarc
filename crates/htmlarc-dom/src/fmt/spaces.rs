#[derive(Default)]
pub(crate) struct Spaces {
    pub nl: u16,
    pub tab: u16,
    pub blank: u16,
    pub other: u16,
}

impl Spaces {
    pub fn count(txt: &str) -> Spaces {
        let mut spaces = Spaces::default();
        for c in txt.chars() {
            if c == '\n' || c == '\r' {
                spaces.nl += 1;
            } else if c == '\t' {
                spaces.tab += 1;
            } else if c == ' ' {
                spaces.blank += 1;
            } else {
                spaces.other += 1;
            }
        }
        spaces
    }

    pub fn remove_formatting(txt: &str) -> String {
        let string = txt.trim_matches(|c| c == '\n' || c == '\t').to_owned();

        let mut new_string = String::new();
        let mut successive = false;

        // replace any successive new lines and/or tabs with a single word space
        for c in string.chars() {
            if c == '\t' || c == '\n' || c == ' ' {
                if !successive {
                    new_string.push(' ');
                    successive = true;
                }
                continue;
            }

            new_string.push(c);
            successive = false;
        }

        new_string
    }

    pub fn is_formatting(&self) -> bool {
        self.other == 0 && self.blank == 0
    }
}
