use tinyvec::ArrayVec;

use crate::{
    dom::{DomInner, NodeIndex, Nodes},
    html::{HtmlAttr, HtmlTag},
    stores::{
        AttributeStoreBuilder, ClassStoreBuilder, DataAttribute, DataAttributeStore, ListIndex,
        StringStack,
    },
};

use super::dom::{DomStack, log, log_list, log_opt_i};

#[derive(Default)]
pub struct DomBuilder {
    pub(crate) nodes: Nodes,
    pub(crate) attrs: AttributeStoreBuilder,
    pub(crate) dataattrs: DataAttributeStore,
    pub(crate) classes: ClassStoreBuilder,
    pub(crate) strings: StringStack,
}

impl DomBuilder {
    pub fn add_text_child(&mut self, tag: HtmlTag, index: NodeIndex, text: &str) -> NodeIndex {
        let range = self.strings.push(text);
        let index = self.nodes.add_as_last_child(index, tag);
        self.nodes.set_text_range(index, range);
        index
    }

    pub fn build(self) -> DomInner {
        DomInner {
            nodes: self.nodes,
            attrs: self.attrs.build(),
            dataattrs: self.dataattrs,
            classes: self.classes.build(),
            strings: self.strings,
        }
    }
}

const MAX_DEPTH: usize = 64;

#[derive(Default)]
pub struct DomBuilderCursor {
    pub dom: DomBuilder,
    pub tag_stack: ArrayVec<[HtmlTag; MAX_DEPTH]>,
    pub index_stack: ArrayVec<[NodeIndex; MAX_DEPTH]>,
    pub attr_list_index: Option<ListIndex>,
    pub dataattr_list_index: Option<ListIndex>,
}

impl DomBuilderCursor {
    fn index(&self) -> NodeIndex {
        *self.index_stack.last().unwrap_or(&NodeIndex::ROOT)
    }
    fn push_index(&mut self, index: NodeIndex) {
        self.index_stack.push(index)
    }
}

impl DomStack for DomBuilderCursor {
    fn _push_tag(&mut self, tag: HtmlTag) {
        self.tag_stack.push(tag);
        self.attr_list_index = None;
        self.dataattr_list_index = None;
        let i = self.dom.nodes.add_as_last_child(self.index(), tag);
        log(i, || format!("push: {tag}"));
        self.push_index(i);
    }

    fn stack_info(&self) -> String {
        self.tag_stack
            .iter()
            .map(HtmlTag::as_str)
            .collect::<Vec<_>>()
            .join(" > ")
    }

    fn _last_tag(&mut self) -> HtmlTag {
        self.tag_stack.last().copied().unwrap_or(HtmlTag::sys_root)
    }

    fn _pop_tag(&mut self) -> Option<HtmlTag> {
        let i = self.index_stack.pop();
        let tag = self.tag_stack.pop();
        self.attr_list_index = None;
        self.dataattr_list_index = None;
        log_opt_i(i, || format!("pop: {tag:?}"));
        tag
    }

    fn add_text_tag(&mut self, tag: HtmlTag, text: &str) {
        let index = self.index();
        self.attr_list_index = None;
        self.dataattr_list_index = None;
        log(index, || format!("add text: {:?}", text));
        self.dom.add_text_child(tag, index, text);
    }

    fn add_attribute_and_value(&mut self, tag: HtmlAttr, val: &str) {
        let index = self.index();
        if tag == HtmlAttr::class {
            log_list(index, Some(""), || format!("add class={val}"));
            let list_index = self.dom.classes.add_class_list(val);
            self.dom
                .nodes
                .set_class_list_index(index, Some(list_index.as_u16()));
        } else if let Some(list_index) = self.attr_list_index {
            self.dom.attrs.add_attribute(list_index, tag, val);
        } else {
            let list_index = self.dom.attrs.new_list(tag, val);
            self.attr_list_index = Some(list_index);
            self.dom
                .nodes
                .set_attr_list_index(index, Some(list_index.as_u16()));
        }
    }

    fn add_data_attribute(&mut self, tag: &str, val: &str) {
        let index = self.index();

        let data_attr = DataAttribute { tag, val };

        if let Some(list_index) = self.dataattr_list_index {
            self.dom.dataattrs.add_attribute(list_index, &data_attr);
        } else {
            let list_index = self.dom.dataattrs.add_list(&data_attr);
            self.dataattr_list_index = Some(list_index);
            self.dom
                .nodes
                .set_data_attr_list_index(index, Some(list_index.as_u16()));
        }
    }
}
