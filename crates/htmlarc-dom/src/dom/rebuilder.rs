use crate::{
    dom::Nodes,
    iters::DomIterator,
    prelude::*,
    stores::{EXT_BASE, ExtTags, NAME_EXT_BASE, RunRebuilder, StringStack, Sym},
};

impl DomInner {
    pub fn rebuild(&self) -> Self {
        // index lookup where index[old_index] => new_index
        let mut indexes: Vec<Option<NodeIndex>> = vec![None; self.nodes.len()];
        // pairs of new & old indexes in the order they are traversed by the element iterator
        let mut ordered_pairs: Vec<(NodeIndex, NodeIndex)> = Vec::with_capacity(self.nodes.len());

        let mut nodes = Nodes::new_based_on(&self.nodes);
        // Class lists and attribute lists are both bare-id runs, so the rebuild drives a
        // `RunRebuilder` over each arena directly. The attribute entries' *names* are
        // symbols shared with class tokens, so they must be unioned into the class rebuilder
        // before the symbol table is compacted (below).
        let mut class_rebuilder =
            RunRebuilder::new(self.class_lists.len(), self.symbols.len() as usize);
        let mut attr_rebuilder =
            RunRebuilder::new(self.attrs.lists.len(), self.attrs.entries.len());
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
                attr_rebuilder.mark_run_used(&self.attrs.lists, i);
            }
            if let Some(i) = self.nodes.class_list_index(old_index) {
                class_rebuilder.mark_run_used(&self.class_lists, i);
            }
            // Extended tag *names* also live in the shared symbol table, so mark each live
            // custom element's name sym into the class rebuilder before the table compacts —
            // the tag-name twin of the attribute-name union below (without it, every custom
            // element's name would dangle after repackage).
            let byte = self.nodes.tag_byte(old_index);
            if byte >= EXT_BASE {
                let sym = self.ext_tags.view().sym_at(old_index, byte);
                class_rebuilder.mark_value_used(sym.as_u16());
            }
        }

        // Union the surviving extended attribute *names* into the class rebuilder's symbol
        // marks: extended names live in the shared symbol table, but only class runs marked
        // it above. Without this every `data-*`/unknown name would be dropped and its
        // `NameSym` would dangle (ADR 0002 §3).
        for entry_id in attr_rebuilder.used_values() {
            let (name, _) = self.attrs.entry(entry_id as u16);
            if name >= NAME_EXT_BASE {
                class_rebuilder.mark_value_used(name - NAME_EXT_BASE);
            }
        }

        let class_rebuilt = class_rebuilder.build(&self.class_lists);
        let class_list_reindex = class_rebuilt.runs_reidx;
        let class_lists = class_rebuilt.runs;
        // The symbol reindex maps each surviving old sym to its dense new id; kept for the
        // node-write loop, which remaps extended tag-name syms through it.
        let sym_reidx = class_rebuilt.value_reidx;
        // Compact the symbol table to the surviving class tokens ∪ extended attr/tag names;
        // the value reindex reproduces the dense numbering, so the run values map for free.
        let symbols = self.symbols.rebuilt(&sym_reidx);

        let attr_rebuilt = attr_rebuilder.build(&self.attrs.lists);
        let attr_list_reindex = attr_rebuilt.runs_reidx;
        // The attribute store compacts its own value table and remaps each surviving entry's
        // name through the symbol reindex (see `AttrStore::rebuilt`).
        let attrs = self
            .attrs
            .rebuilt(&attr_rebuilt.value_reidx, &sym_reidx, attr_rebuilt.runs);

        // Re-derive the extended-tag vocab from scratch in new-traversal order — `encode` is
        // shared with parse, so rebuild ≡ parse: freed vocab slots compact and overflow tags
        // promote into the vocab automatically (ADR 0002 §4).
        let mut ext_tags = ExtTags::default();

        for (new_index, old_index) in ordered_pairs {
            let tag = self.nodes.tag(old_index);
            let byte = self.nodes.tag_byte(old_index);

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

            // An extended tag's byte is a per-document vocab index: remap its name sym through
            // `sym_reidx` and re-encode against the fresh vocab. Standard bytes copy verbatim.
            let new_byte = if byte >= EXT_BASE {
                let old_sym = self.ext_tags.view().sym_at(old_index, byte);
                let new_sym = sym_reidx[old_sym.as_u16() as usize]
                    .expect("a live extended tag name must have been marked");
                ext_tags.encode(Sym::from(new_sym), new_index)
            } else {
                byte
            };
            nodes.add_node_byte(new_byte, parent_index, prev_sibling, next_sibling);

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

                // Emptying a class or attribute run drops the node's pointer at mutation
                // time, so every surviving pointer was marked and reindexes to `Some`.
                if let Some(list_index) = self.nodes.class_list_index(old_index) {
                    nodes
                        .set_class_list_index(new_index, class_list_reindex[list_index.as_usize()]);
                }
                if let Some(list_index) = self.nodes.attr_list_index(old_index) {
                    nodes.set_attr_list_index(new_index, attr_list_reindex[list_index.as_usize()]);
                }
            }
        }
        DomInner {
            nodes,
            attrs,
            symbols,
            class_lists,
            ext_tags,
            strings,
        }
    }
}
