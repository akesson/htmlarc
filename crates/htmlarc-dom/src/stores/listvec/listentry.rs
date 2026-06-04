use rkyv::{Archive, Deserialize, Serialize};

#[derive(Hash, Clone, Copy, Archive, Serialize, Deserialize)]
pub(crate) struct ListEntry {
    pub(crate) info: ListInfo,
    pub(crate) value: u16,
}

impl ListEntry {
    pub(crate) fn tail(value: u16) -> Self {
        let info = ListInfo::tail();
        Self { info, value }
    }

    pub(crate) fn new_head(value: u16) -> Self {
        let info = ListInfo::head();
        Self { info, value }
    }

    pub(crate) fn has_value(&self) -> bool {
        self.value != u16::MAX
    }

    pub(crate) fn is_empty_head(&self) -> bool {
        self.info.is_head() && !self.info.has_next() && !self.has_value()
    }

    pub(crate) fn unset(&mut self) {
        self.info.unset_next();
        self.value = u16::MAX;
    }
}

/// If first bit is 1, then it's the start of a list.
/// The remaining bits carries the index of next value,
/// and if those bits are 0 then there is no next set.
#[derive(Hash, Clone, Copy, Archive, Serialize, Deserialize)]
pub(crate) struct ListInfo(u16);

impl ListInfo {
    pub(crate) fn head() -> Self {
        Self(0b1000_0000_0000_0000)
    }

    pub(crate) fn tail() -> Self {
        Self(0)
    }

    pub(crate) fn next(&self) -> Option<u16> {
        let num = self.0 & 0b0111_1111_1111_1111;
        (num != 0).then_some(num)
    }

    pub(crate) fn has_next(&self) -> bool {
        (self.0 & 0b0111_1111_1111_1111) != 0
    }

    pub(crate) fn set_next(&mut self, value: u16) {
        debug_assert_ne!(value, 0, "0 is not a valid index");
        self.0 &= 0b1000_0000_0000_0000;
        self.0 |= value;
    }

    pub(crate) fn set_next_opt(&mut self, value: Option<u16>) {
        if let Some(value) = value {
            self.set_next(value);
        } else {
            self.unset_next();
        }
    }

    pub(crate) fn is_head(&self) -> bool {
        self.0 & 0b1000_0000_0000_0000 != 0
    }

    pub(crate) fn unset_next(&mut self) {
        if self.is_head() {
            self.0 = 0b1000_0000_0000_0000;
        } else {
            self.0 = 0;
        }
    }

    pub(crate) fn reindexed(&self, reindex: &[Option<u16>]) -> Self {
        let num = self.0 & 0b0111_1111_1111_1111;
        let head = self.0 & 0b1000_0000_0000_0000;
        let new_num = reindex[num as usize].expect("Invalid reindex");
        Self(new_num | head)
    }
}

#[test]
fn listinfo_head() {
    let mut info = ListInfo::head();
    assert_eq!(info.next(), None);
    assert!(info.is_head());

    info.set_next(1);
    assert_eq!(info.next(), Some(1));
    assert!(info.is_head());

    info.unset_next();
    assert_eq!(info.next(), None);
    assert!(info.is_head());
}

#[test]
fn listinfo_tail() {
    let mut info = ListInfo::tail();
    assert_eq!(info.next(), None);
    assert!(!info.is_head());

    info.set_next(1);
    assert_eq!(info.next(), Some(1));
    assert!(!info.is_head());

    info.unset_next();
    assert_eq!(info.next(), None);
    assert!(!info.is_head());
}
