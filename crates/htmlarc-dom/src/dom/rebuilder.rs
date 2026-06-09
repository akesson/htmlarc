use crate::{
    dom::Nodes,
    iters::DomIterator,
    prelude::*,
    stores::{AttributeReBuilder, ClassReBuilder, DataAttributeRebuilder, StringStack},
};

impl DomInner {
    pub fn rebuild(&self) -> Self {
        // index lookup where index[old_index] => new_index
        let mut indexes: Vec<Option<NodeIndex>> = vec![None; self.nodes.len()];
        // pairs of new & old indexes in the order they are traversed by the element iterator
        let mut ordered_pairs: Vec<(NodeIndex, NodeIndex)> = Vec::with_capacity(self.nodes.len());

        let mut attr_rebuilder = AttributeReBuilder::new(&self.attrs);
        let mut nodes = Nodes::new_based_on(&self.nodes);
        let mut class_rebuilder = ClassReBuilder::new(&self.classes);
        let mut dataattrs_rebuilder = DataAttributeRebuilder::new(&self.dataattrs);
        let mut strings = StringStack::with_capacity(self.strings.size());

        let iter = HtmlElement::new(self, NodeIndex::ROOT)
            .forwards()
            .set_include_text()
            .set_include_comment();

        // the iter doesn't do root
        indexes[0] = Some(NodeIndex::ROOT);
        ordered_pairs.push((NodeIndex::ROOT, NodeIndex::ROOT));

        for (new_index, el) in iter.enumerate() {
            let new_index = NodeIndex::new(new_index as u32 + 1);
            let old_index = el.index();
            indexes[old_index.as_usize()] = Some(new_index);
            ordered_pairs.push((new_index, old_index));
            if let Some(i) = self.nodes.attr_list_index(old_index) {
                attr_rebuilder.mark_list_used(&self.attrs, i);
            }
            if let Some(i) = self.nodes.class_list_index(old_index) {
                class_rebuilder.mark_list_used(&self.classes, i);
            }
            if let Some(i) = self.nodes.data_attr_list_index(old_index) {
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
                .and_then(|i| indexes[i.as_usize()]);
            let prev_sibling = self
                .nodes
                .prev_sibling_index(old_index)
                .and_then(|i| indexes[i.as_usize()]);
            let next_sibling = self
                .nodes
                .next_sibling_index(old_index)
                .and_then(|i| indexes[i.as_usize()]);

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
                    .and_then(|i| indexes[i.as_usize()]);
                nodes.set_first_child_index(new_index, first_child);

                let last_child = self
                    .nodes
                    .last_child_index(old_index)
                    .and_then(|i| indexes[i.as_usize()]);
                nodes.set_last_child_index(new_index, last_child);

                // A reindex of `None` means the list was emptied by a mutation: its head
                // slot is now an empty head that `mark_list_used` intentionally skips, so it
                // is not carried into the rebuilt store. Drop the node's stale pointer
                // instead of unwrapping it.
                if let Some(list_index) = self.nodes.class_list_index(old_index) {
                    nodes
                        .set_class_list_index(new_index, class_list_reindex[list_index.as_usize()]);
                }
                if let Some(list_index) = self.nodes.attr_list_index(old_index) {
                    nodes.set_attr_list_index(new_index, attr_list_reindex[list_index.as_usize()]);
                }
                if let Some(list_index) = self.nodes.data_attr_list_index(old_index) {
                    nodes.set_data_attr_list_index(
                        new_index,
                        dataattr_list_reindex[list_index.as_usize()],
                    );
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
