use std::{cell::RefCell, fmt::Display};

use tinyvec::TinyVec;

use super::{DBG, MAX_DEPTH};
use crate::dom::NodesView;
#[derive(Clone)]
pub struct SimpleStack(RefCell<TinyVec<[u16; 32]>>);

impl SimpleStack {
    pub fn from_root_to_element(nodes: NodesView, mut index: u16) -> Self {
        let mut stack = TinyVec::new();
        stack.push(index);
        while let Some(parent) = nodes.parent_index(index) {
            DBG.then(|| println!("parent {}", parent));
            index = parent;
            stack.push(index);
        }
        stack.reverse();
        DBG.then(|| println!("root_to_element   {}", stack));
        Self(RefCell::new(stack))
    }

    pub fn push(&self, index: u16) {
        let stack = &mut *self.0.borrow_mut();
        stack.push(index);
        DBG.then(|| println!("push {stack}"));
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn pop(&self) -> Option<u16> {
        let stack = &mut *self.0.borrow_mut();
        let popped = stack.pop();
        DBG.then(|| println!("pop {stack}"));
        popped
    }

    pub fn last(&self) -> Option<u16> {
        self.0.borrow().last().copied()
    }
}

#[derive(Default, Clone, Copy)]
struct Visited {
    /// index of the node
    index: u16,
    /// this is the nth sibling
    ordinal: u16,
}
impl Display for Visited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.index, self.ordinal)
    }
}

pub enum VisitedStatus {
    Changed(u16),
    Same(u16),
    StackEmpty,
}

/// The VisitedStack keeps track of already visited nodes and their
/// sibling ordinal
#[derive(Clone)]
pub struct VisitedStack(TinyVec<[Visited; MAX_DEPTH]>);

impl Display for VisitedStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl VisitedStack {
    pub fn from_element(nodes: NodesView, index: u16) -> Self {
        let mut stack = TinyVec::new();
        let ordinal = Self::ordinal(nodes, index);
        stack.push(Visited { index, ordinal });
        DBG.then(|| println!("from_element      {stack}"));
        Self(stack)
    }

    pub fn from_root_to_element(nodes: NodesView, mut index: u16) -> Self {
        let mut me = Self::from_element(nodes, index);
        while let Some(parent) = nodes.parent_index(index) {
            DBG.then(|| println!("parent {}", parent));
            index = parent;
            let ordinal = Self::ordinal(nodes, index);
            me.0.push(Visited { index, ordinal });
        }
        me.0.reverse();
        DBG.then(|| println!("root_to_element   {}", me.0));
        me
    }

    fn ordinal(nodes: NodesView, mut index: u16) -> u16 {
        let mut ord = 0;
        while let Some(i) = nodes.prev_sibling_index(index) {
            index = i;
            ord += 1;
        }
        ord
    }

    pub fn push_first_child(&mut self, index: u16) {
        self.0.push(Visited { index, ordinal: 0 });
        DBG.then(|| println!("push_first_child  {}", self.0));
    }

    pub fn set_next_sibling(&mut self, index: u16) {
        if let Some(visited) = self.0.last_mut() {
            visited.index = index;
            visited.ordinal += 1;
            DBG.then(|| println!("set_next_sibling  {}", self.0));
        }
    }
    /// The last item on the stack is checked to see if it's parent is the same
    /// as the previous entry in the stack. If it's not then we try to find the new
    /// element by going to the parent and then finding the nth child of that parent
    pub fn last_updated(&mut self, nodes: NodesView) -> VisitedStatus {
        loop {
            let len = self.0.len();
            if len == 0 {
                return VisitedStatus::StackEmpty;
            } else if len == 1 {
                // no check necessary when we are at the root
                return VisitedStatus::Same(self.0[len - 1].index);
            }
            let last_index = len - 1;
            let element = self.0[last_index];
            let stack_parent = self.0[last_index - 1].index;
            // let element_parent = nodes.parent_index(element.index);
            if Some(stack_parent) == nodes.parent_index(element.index) {
                // the element's parent is the same
                return VisitedStatus::Same(element.index);
            } else if let Some(new_index) = Self::nth_child_of(nodes, stack_parent, element.ordinal) {
                // the element was removed or replaced, but we found another element at the same position
                // relative to the parent
                self.0[last_index].index = new_index;
                // println!(
                //     "changed {} {} with parent {:?} and stack_parent {stack_parent} to {new_index}. Stack is {}",
                //     element.index,
                //     dom.nodes.tag(element.index),
                //     dom.nodes.parent_index(element.index),
                //     self.0,
                // );
                return VisitedStatus::Changed(new_index);
            } else {
                // println!("popped");
                // the element was removed and there is no element replacing it
                self.0.pop();
            }
        }
    }

    fn nth_child_of(nodes: NodesView, parent: u16, n: u16) -> Option<u16> {
        let mut index = nodes.first_child_index(parent)?;
        for _ in 0..n {
            index = nodes.next_sibling_index(index)?;
        }
        Some(index)
    }

    pub fn pop(&mut self) -> Option<u16> {
        let popped = self.0.pop().map(|visited| visited.index);
        DBG.then(|| println!("pop               {}", self.0));
        popped
    }

    pub fn last(&self) -> Option<u16> {
        self.0.last().map(|visited| visited.index)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}
