use crate::{
    dom::Nodes,
    iters::DomIterator,
    prelude::*,
    stores::{AttributeReBuilder, ClassReBuilder, DataAttributeRebuilder, StringStack},
};

impl DomInner {
    pub fn rebuild(&self) -> Self {
        // index lookup where index[old_index] => new_index
        let mut indexes: Vec<Option<u16>> = vec![None; self.nodes.len()];
        // pairs of new & old indexes in the order they are traversed by the element iterator
        let mut ordered_pairs: Vec<(u16, u16)> = Vec::with_capacity(self.nodes.len());

        let mut attr_rebuilder = AttributeReBuilder::new(&self.attrs);
        let mut nodes = Nodes::new_based_on(&self.nodes);
        let mut class_rebuilder = ClassReBuilder::new(&self.classes);
        let mut dataattrs_rebuilder = DataAttributeRebuilder::new(&self.dataattrs);
        let mut strings = StringStack::with_capacity(self.strings.size());

        let iter = HtmlElement::new(self, 0)
            .forwards()
            .set_include_text()
            .set_include_comment();

        // the iter doesn't do root
        indexes[0] = Some(0);
        ordered_pairs.push((0, 0));

        for (new_index, el) in iter.enumerate() {
            let new_index = new_index as u16 + 1;
            let old_index = el.index() as usize;
            indexes[old_index] = Some(new_index);
            ordered_pairs.push((new_index, old_index as u16));
            if let Some(i) = self.nodes.attr_list_index(old_index as u16) {
                attr_rebuilder.mark_list_used(&self.attrs, i);
            }
            if let Some(i) = self.nodes.class_list_index(old_index as u16) {
                class_rebuilder.mark_list_used(&self.classes, i);
            }
            if let Some(i) = self.nodes.data_attr_list_index(old_index as u16) {
                dataattrs_rebuilder.mark_list_used(&self.dataattrs, i);
            }
        }

        let (attr_list_reindex, attrs) = attr_rebuilder.build(&self.attrs);
        let (class_list_reindex, classes) = class_rebuilder.build(&self.classes);
        let (dataattr_list_reindex, dataattrs) = dataattrs_rebuilder.build(&self.dataattrs);

        for (new_index, old_index) in ordered_pairs {
            let tag = self.nodes.tag(old_index);

            let parent_index = self
                .nodes
                .parent_index(old_index)
                .and_then(|i| indexes[i as usize]);
            let prev_sibling = self
                .nodes
                .prev_sibling_index(old_index)
                .and_then(|i| indexes[i as usize]);
            let next_sibling = self
                .nodes
                .next_sibling_index(old_index)
                .and_then(|i| indexes[i as usize]);

            nodes.add_node(tag, parent_index, prev_sibling, next_sibling);

            if tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment {
                let text = self.string_at(old_index);
                let range = strings.push(text);
                nodes.set_text_range(new_index, range);
            } else if tag == HtmlTag::sys_deleted {
                panic!("No deleted tags should be transferred");
            } else {
                let first_child = self
                    .nodes
                    .first_child_index(old_index)
                    .and_then(|i| indexes[i as usize]);
                nodes.set_first_child_index(new_index, first_child);

                let last_child = self
                    .nodes
                    .last_child_index(old_index)
                    .and_then(|i| indexes[i as usize]);
                nodes.set_last_child_index(new_index, last_child);

                if let Some(list_index) = self.nodes.class_list_index(old_index) {
                    let new_list = class_list_reindex[list_index.as_usize()].unwrap();
                    nodes.set_class_list_index(new_index, Some(new_list));
                }
                if let Some(list_index) = self.nodes.attr_list_index(old_index) {
                    let new_list = attr_list_reindex[list_index.as_usize()].unwrap();
                    nodes.set_attr_list_index(new_index, Some(new_list));
                }
                if let Some(list_index) = self.nodes.data_attr_list_index(old_index) {
                    let new_list = dataattr_list_reindex[list_index.as_usize()].unwrap();
                    nodes.set_data_attr_list_index(new_index, Some(new_list));
                }
            }
        }
        DomInner {
            nodes,
            attrs,
            dataattrs,
            classes,
            strings,
        }
    }
}
