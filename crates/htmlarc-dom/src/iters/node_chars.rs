use super::DBG;

#[derive(Default)]
pub(super) struct NodeChars {
    string: Vec<char>,
    cursor: Option<usize>,
    reverse: bool,
}

impl NodeChars {
    pub fn rev() -> Self {
        Self {
            reverse: true,
            ..Default::default()
        }
    }

    pub fn reset(&mut self, string: Option<String>) {
        self.cursor = None;

        if let Some(string) = string {
            self.string = string.chars().collect();
        } else {
            self.string.clear();
        }
    }

    pub fn current_char(&self) -> Option<char> {
        self.cursor.and_then(|i| self.string.get(i).copied())
    }

    pub fn current_index(&self) -> Option<usize> {
        self.cursor
    }

    pub fn next(&mut self) -> Option<char> {
        let new_index = next_index(self.cursor, self.reverse, self.string.len())?;
        let char = self.string.get(new_index)?;
        self.cursor = Some(new_index);
        Some(*char)
    }

    pub fn remove_current(&mut self) {
        if let Some(index) = self.cursor {
            self.string.remove(index);

            if !self.reverse {
                let new_index = index as isize - 1;
                self.cursor = if new_index < 0 {
                    None
                } else {
                    Some(new_index as usize)
                };
            }
        } else {
            DBG.then(|| println!("no current char to remove"));
        }
    }

    pub fn replace_current(&mut self, new_char: char) {
        if let Some(index) = self.cursor {
            self.string[index] = new_char;
        } else {
            DBG.then(|| println!("no current char to replace"));
        }
    }

    pub fn insert(&mut self, new_char: char, before: bool) {
        if let Some(index) = self.cursor {
            let (insert_index, new_index) = if before {
                (index, index + 1)
            } else {
                (index + 1, index)
            };

            self.string.insert(insert_index, new_char);
            self.cursor = Some(new_index);
        } else {
            DBG.then(|| println!("no current char to insert before"));
        }
    }

    pub fn string(&self) -> String {
        self.string.iter().collect()
    }
}

fn next_index(current_index: Option<usize>, reverse: bool, len: usize) -> Option<usize> {
    if reverse {
        if let Some(index) = current_index {
            let new_index = index as isize - 1;
            if new_index < 0 {
                None
            } else {
                Some(new_index as usize)
            }
        } else {
            Some((len as isize - 1) as usize)
        }
    } else if let Some(index) = current_index {
        Some(index + 1)
    } else {
        Some(0)
    }
}
