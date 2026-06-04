use super::{ListIndex, ListVec};

pub(crate) struct ListRebuilder {
    lists_reidx: Vec<Option<u16>>,
    value_reidx: Vec<Option<u16>>,
}

impl ListRebuilder {
    pub fn new(max_list_size: usize, max_values_size: usize) -> Self {
        Self {
            lists_reidx: vec![None; max_list_size],
            value_reidx: vec![None; max_values_size],
        }
    }

    pub fn mark_list_used(&mut self, lists: &ListVec, index: ListIndex) {
        let Some(mut index) = lists.head_index_at(index).map(|i| i.as_usize()) else {
            return;
        };
        loop {
            let entry = lists.vec[index];
            self.lists_reidx[index] = Some(1);
            self.value_reidx[entry.value as usize] = Some(1);
            let Some(next) = entry.info.next() else {
                break;
            };
            index = next as usize;
        }
    }

    pub fn build(self, lists: &ListVec) -> ListRebuilt {
        let Self {
            mut lists_reidx,
            mut value_reidx,
        } = self;
        calculate_indexes(&mut value_reidx);
        calculate_indexes(&mut lists_reidx);

        let lists = lists.rebuild(&value_reidx, &lists_reidx);
        ListRebuilt {
            lists_reidx,
            value_reidx,
            lists,
        }
    }
}

pub(crate) struct ListRebuilt {
    pub(crate) lists_reidx: Vec<Option<u16>>,
    pub(crate) value_reidx: Vec<Option<u16>>,
    pub(crate) lists: ListVec,
}

fn calculate_indexes(vec: &mut [Option<u16>]) {
    for (index, val) in vec.iter_mut().flatten().enumerate() {
        *val = index as u16;
    }
}
