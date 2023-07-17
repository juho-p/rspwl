use std::cell::RefCell;
use std::iter::Iterator;
use std::rc::{Rc, Weak};
use std::sync::atomic;
use std::collections::HashMap;

use crate::types::{NodeId, Result, Rect};

static ID_GEN: atomic::AtomicU32 = atomic::AtomicU32::new(1);

fn id_gen() -> NodeId {
    ID_GEN.fetch_add(1, atomic::Ordering::Relaxed)
}

#[derive(Debug)]
pub enum N<T> {
    Placeholder,
    Leaf(Leaf<T>),
    Split(Split<T>),
}

#[derive(Debug)]
pub struct Node<T> {
    pub id: NodeId,
    parent: RefCell<Option<Weak<Node<T>>>>,
    pub n: RefCell<N<T>>,
    pub rect: RefCell<Rect>,
}

impl<T> Node<T> {
    pub fn root(self: Rc<Self>) -> Rc<Node<T>> {
        let mut n = self.clone();
        while let Some(p) = n.parent() {
            n = p.clone();
        }
        n
    }

    fn parent(&self) -> Option<Rc<Node<T>>> {
        self.parent.borrow().as_ref().map(|x| x.upgrade().unwrap())
    }

    fn ancestors(&self) -> AncestorIter<T> {
        AncestorIter {
            next: self.parent(),
            curr_id: self.id,
        }
    }

    pub fn self_and_descendants(self: Rc<Self>) -> impl Iterator<Item = Rc<Node<T>>> {
        let top = self.id;
        DescendantIter {
            next: Some(self),
            top,
        }
    }
}

pub struct Leaf<Content> {
    pub content: Content,
}

impl<Content> std::fmt::Debug for Leaf<Content> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Leaf { ... }")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    H,
    V,
}

#[derive(Debug)]
pub struct Split<T> {
    pub dir: SplitDir,
    pub a: Rc<Node<T>>,
    pub b: Rc<Node<T>>,
}

fn invalid_tree_op() -> ! {
    panic!("Tree state is invalid")
}

pub fn create_root<T>() -> Rc<Node<T>> {
    Rc::new(Node {
        id: 0,
        parent: RefCell::new(None),
        n: RefCell::new(N::Placeholder),
        rect: RefCell::new(Rect::default()),
    })
}

pub fn add_leaf<T>(
    target_node: Rc<Node<T>>,
    content: impl FnOnce(NodeId) -> T,
    split_dir: SplitDir,
) -> (Rc<Node<T>>, Rc<Node<T>>) {
    let is_placeholder = match &*target_node.n.borrow() {
        N::Placeholder => true,
        _ => false,
    };

    if is_placeholder {
        let new_n = N::Leaf(Leaf {
            content: content(target_node.id),
        });
        *target_node.n.borrow_mut() = new_n;
        (target_node.clone(), target_node)
    } else {
        let id = id_gen();
        let new_n = N::Leaf(Leaf {
            content: content(id),
        });
        let new_node = Rc::new(Node {
            id,
            parent: RefCell::new(Some(Rc::downgrade(&target_node))),
            n: RefCell::new(new_n),
            rect: RefCell::new(Rect::default()),
        });
        let split = split_from(target_node, new_node.clone(), split_dir);

        (split, new_node)
    }
}

fn split_from<T>(tree_node: Rc<Node<T>>, new_node: Rc<Node<T>>, dir: SplitDir) -> Rc<Node<T>> {
    let parent = tree_node.parent();

    let new_split = Rc::new(Node {
        id: id_gen(),
        parent: tree_node.parent.clone(),
        n: RefCell::new(N::Split(Split {
            dir,
            a: tree_node.clone(),
            b: new_node.clone(),
        })),
        rect: RefCell::new(Rect::default()),
    });

    *new_node.parent.borrow_mut() = Some(Rc::downgrade(&new_split));
    *tree_node.parent.borrow_mut() = Some(Rc::downgrade(&new_split));

    if let Some(p) = parent {
        replace_child(&p, &tree_node, new_split.clone());
    }

    new_split
}

/// remove node from tree, return new (possibly changed) root
pub fn remove_from_tree<T>(node: Rc<Node<T>>) -> Result<Rc<Node<T>>> {
    fn sibling<T>(s: &Split<T>, child: &Node<T>) -> Rc<Node<T>> {
        if s.a.id == child.id {
            s.b.clone()
        } else if s.b.id == child.id {
            s.a.clone()
        } else {
            invalid_tree_op();
        }
    }

    match &*node.n.borrow() {
        N::Leaf(_) => (),
        _ => return Err("Only leaf can be removed".to_string()),
    }

    if let Some(ref parent) = node.parent() {
        match &*parent.n.borrow() {
            N::Split(split) => {
                let sibling = sibling(&split, &node);
                replace_node(parent, sibling.clone());
                Ok(sibling.root())
            }
            _ => invalid_tree_op(),
        }
    } else {
        *node.n.borrow_mut() = N::Placeholder;
        Ok(node.root())
    }
}

fn replace_node<T>(from: &Rc<Node<T>>, to: Rc<Node<T>>) {
    let parent = from.parent();
    if let Some(ref parent) = parent {
        replace_child(&parent, from, to.clone());
    }

    *to.parent.borrow_mut() = parent.map(|x| Rc::downgrade(&x));
}

fn replace_child<T>(parent: &Rc<Node<T>>, from: &Rc<Node<T>>, to: Rc<Node<T>>) {
    match &mut *parent.n.borrow_mut() {
        N::Split(s) => {
            if s.a.id == from.id {
                s.a = to;
            } else if s.b.id == from.id {
                s.b = to;
            } else {
                panic!("Invalid child replace");
            }
        }
        N::Leaf(_) | N::Placeholder => invalid_tree_op(),
    }
}

pub fn swap<T>(node1: Rc<Node<T>>, node2: Rc<Node<T>>) {
    let Some(parent1) = node1.parent() else { return; };
    let Some(parent2) = node2.parent() else { return; };

    if parent1.id == parent2.id {
        std::mem::drop(parent2);
        match &mut *parent1.n.borrow_mut() {
            N::Placeholder | N::Leaf(_) => invalid_tree_op(),
            N::Split(s) => std::mem::swap(&mut s.a, &mut s.b),
        }
    } else {
        replace_child(&parent1, &node1, node2.clone());
        *node1.parent.borrow_mut() = Some(Rc::downgrade(&parent2));
        replace_child(&parent2, &node2, node1);
        *node2.parent.borrow_mut() = Some(Rc::downgrade(&parent1));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Right,
    Down,
    Left,
}

pub fn nodes_to_direction<T>(start_node: &Node<T>, direction: Direction) -> HashMap<NodeId, Rc<Node<T>>> {
    let split_dir = match direction {
        Direction::Up | Direction::Down => SplitDir::H,
        Direction::Left | Direction::Right => SplitDir::V,
    };
    let up_or_left = direction == Direction::Up || direction == Direction::Left;

    // find our way to the up
    let ancestor_sibling = start_node.ancestors()
        .find_map(|(parent, nodeid)| match &*parent.n.borrow() {
            N::Split(s) => if s.dir == split_dir {
                let dir_child = if up_or_left { &s.a } else { &s.b };
                if dir_child.id == nodeid {
                    None
                } else {
                    Some(dir_child.clone())
                }
            } else {
                None
            }
            _ => None,
        });

    // descend in opposite direction
    let mut collected = HashMap::new();

    if let Some(ancestor_sibling) = ancestor_sibling {
        let mut search = Some(ancestor_sibling.clone());
        while let Some(current_search) = search {
            collected.insert(current_search.id, current_search.clone());
            let next = match &*current_search.n.borrow() {
                N::Leaf(_) | N::Placeholder => {
                    // up and next
                    current_search.ancestors()
                        .take_while(|x| x.1 != ancestor_sibling.id)
                        .find_map(|(node, child_id)| {
                            match &*node.n.borrow() {
                                N::Leaf(_) | N::Placeholder => None,
                                N::Split(split) => {
                                    if split.dir == split_dir {
                                        None
                                    } else if child_id == split.a.id {
                                        Some(split.b.clone())
                                    } else {
                                        None
                                    }
                                }
                            }
                        })
                }
                N::Split(split) => {
                    if split.dir == split_dir {
                        // reverse direction
                        Some(if up_or_left { split.b.clone() } else { split.a.clone() })
                    } else {
                        Some(split.a.clone())
                    }
                }
            };
            search = next;
        }
    }

    collected
}

pub struct AncestorIter<T> {
    next: Option<Rc<Node<T>>>,
    curr_id: NodeId,
}

pub struct DescendantIter<T> {
    next: Option<Rc<Node<T>>>,
    top: NodeId,
}

impl<T> Iterator for AncestorIter<T> {
    type Item = (Rc<Node<T>>, NodeId);

    fn next(&mut self) -> Option<Self::Item> {
        match self.next.take() {
            Some(curr) => {
                self.next = curr.parent();
                let prev_id = self.curr_id;
                self.curr_id = curr.id;
                Some((curr, prev_id))
            }
            None => None
        }
    }
}

impl<T> Iterator for DescendantIter<T> {
    type Item = Rc<Node<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = if let Some(x) = &self.next {
            x.clone()
        } else {
            return None;
        };

        self.next = match &*curr.n.borrow() {
            N::Leaf(_) | N::Placeholder => {
                curr.clone().ancestors()
                    .take_while(|x| x.1 != self.top)
                    .find_map(|(node, child_id)| {
                        match &*node.n.borrow() {
                            N::Leaf(_) | N::Placeholder => None,
                            N::Split(split) => {
                                if child_id == split.a.id {
                                    Some(split.b.clone())
                                } else {
                                    None
                                }
                            }
                        }
                    })
            }
            N::Split(split) => Some(split.a.clone()),
        };

        Some(curr)
    }
}

#[test]
fn test_add() {
    let r = create_root::<&'static str>();

    let (first, t) = add_leaf(r.clone(), |_| "first", SplitDir::V);
    assert_eq!(first.id, t.id);
    assert_eq!(1, first.clone().self_and_descendants().count());

    let (firstsplit, a) = add_leaf(first.clone(), |_| "a", SplitDir::H);
    assert_eq!(firstsplit.id, a.clone().root().id);
    assert_eq!(firstsplit.id, firstsplit.clone().root().id);
    assert_ne!(firstsplit.id, a.id);
    assert!(match &*firstsplit.n.borrow() {
        N::Split(_) => true,
        _ => false,
    });
    assert_eq!(
        vec![firstsplit.id, first.id, a.id],
        firstsplit
            .clone()
            .self_and_descendants()
            .map(|x| x.id)
            .collect::<Vec<NodeId>>()
    );

    let neighbors = nodes_to_direction(
        &firstsplit.clone().self_and_descendants().skip(1).next().unwrap(),
        Direction::Down
    );
    assert_eq!(1, neighbors.len());
    assert_eq!(firstsplit.clone().self_and_descendants().skip(2).next().unwrap().id, neighbors.iter().next().unwrap().1.id);

    let (secondsplit, b) = add_leaf(firstsplit.clone(), |_| "b", SplitDir::H);
    assert_eq!(secondsplit.id, a.clone().root().id);
    assert_eq!(
        vec![secondsplit.id, firstsplit.id, first.id, a.id, b.id],
        secondsplit
            .clone()
            .self_and_descendants()
            .map(|x| x.id)
            .collect::<Vec<NodeId>>()
    );

    let (thirdsplit, c) = add_leaf(b.clone(), |_| "c", SplitDir::H);
    assert_eq!(secondsplit.id, a.clone().root().id);
    assert_eq!(
        vec![
            secondsplit.id,
            firstsplit.id,
            first.id,
            a.id,
            thirdsplit.id,
            b.id,
            c.id
        ],
        secondsplit
            .clone()
            .self_and_descendants()
            .map(|x| x.id)
            .collect::<Vec<NodeId>>()
    );
}

#[test]
fn test_remove() {
    let r = create_root::<&'static str>();
    let (first, _t1) = add_leaf(r.clone(), |_| "first", SplitDir::V);
    let (firstsplit, a) = add_leaf(first.clone(), |_| "a", SplitDir::H);
    let (_t2, b) = add_leaf(firstsplit.clone(), |_| "b", SplitDir::H);

    let root1 = a.clone().root();

    remove_from_tree(a).unwrap();

    assert_eq!(b.clone().root().id, root1.id);
    assert_eq!(
        vec![root1.id, first.id, b.id],
        root1
            .self_and_descendants()
            .map(|x| x.id)
            .collect::<Vec<NodeId>>()
    );

    remove_from_tree(first).unwrap();

    assert_eq!(b.clone().root().id, b.id);

    assert!(matches!(&*b.n.borrow(), N::Leaf(_)));

    remove_from_tree(b.clone()).unwrap();

    assert!(!matches!(&*b.n.borrow(), N::Leaf(_)));
}
