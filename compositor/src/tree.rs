use std::cell::{Ref, RefCell};
use std::iter::Iterator;
use std::ops::Deref;
use std::rc::{Rc, Weak};
use std::sync::atomic;

use crate::types::{Result, NodeId};

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
}

pub struct DescendantIter<T> {
    next: Option<Rc<Node<T>>>,
}

impl<T> Iterator for DescendantIter<T> {
    type Item = Rc<Node<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = if let Some(x) = &self.next { x.clone() } else { return None };

        self.next = match &*curr.n.borrow() {
            N::Placeholder => None,
            N::Leaf(_) => {
                let mut new_next = None;
                let mut t = curr.clone();
                while let Some(up) = t.parent() {
                    match &*up.n.borrow() {
                        N::Split(s) => {
                            if t.id == s.a.id {
                                new_next = Some(s.b.clone());
                                break;
                            }
                        }
                        _ => invalid_tree_op(),
                    }
                    t = up;
                }
                new_next
            }
            N::Split(split) => {
                Some(split.a.clone())
            }
        };

        Some(curr)
    }
}

impl<T> Node<T> {
    pub fn root(self: Rc<Node<T>>) -> Rc<Node<T>> {
        let mut n = self.clone();
        while let Some(p) = n.parent() {
            n = p.clone();
        }
        n
    }

    fn parent(&self) -> Option<Rc<Node<T>>> {
        self.parent.borrow().as_ref().map(|x| x.upgrade().unwrap())
    }

    pub fn self_and_descendants(self: Rc<Node<T>>) -> impl Iterator<Item = Rc<Node<T>>> {
        DescendantIter {
            next: Some(self.clone())
        }
    }

    pub fn as_content(&self) -> Option<LeafContentRef<T>> {
        match &*self.n.borrow() {
            N::Leaf(_) => Some(LeafContentRef {
                n: self.n.borrow(),
            }),
            _ => None
        }
    }
}

pub struct LeafContentRef<'a, T> {
    n: Ref<'a, N<T>>,
}

impl<'a, T> Deref for LeafContentRef<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match &*self.n {
            N::Leaf(l) => &l.content,
            _ => panic!("ouch"),
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

#[derive(Debug, Clone, Copy)]
pub enum Dir {
    H,
    V,
}

#[derive(Debug)]
pub struct Split<T> {
    pub dir: Dir,
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
    })
}

pub fn add_leaf<T>(target_node: Rc<Node<T>>, content: impl FnOnce(NodeId) -> T, split_dir: Dir) -> (Rc<Node<T>>, Rc<Node<T>>) {
    let is_placeholder = match &*target_node.n.borrow() {
        N::Placeholder => true,
        _ => false,
    };


    if is_placeholder {
        let new_n = N::Leaf(Leaf { content: content(target_node.id) });
        *target_node.n.borrow_mut() = new_n;
        (target_node.clone(), target_node)
    } else {
        let id = id_gen();
        let new_n = N::Leaf(Leaf { content: content(id) });
        let new_node = Rc::new(Node {
            id,
            parent: RefCell::new(Some(Rc::downgrade(&target_node))),
            n: RefCell::new(new_n),
        });
        let split = split_from(target_node, new_node.clone(), split_dir);

        (split, new_node)
    }
}

fn split_from<T>(tree_node: Rc<Node<T>>, new_node: Rc<Node<T>>, dir: Dir) -> Rc<Node<T>> {
    let parent = tree_node.parent();

    let new_split = Rc::new(Node {
        id: id_gen(),
        parent: tree_node.parent.clone(),
        n: RefCell::new(N::Split(Split {
            dir,
            a: tree_node.clone(),
            b: new_node.clone(),
        })),
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

#[test]
fn test_add() {
    let r = create_root::<&'static str>();

    let (first, t) = add_leaf(r.clone(), |_| "first", Dir::V);
    assert_eq!(first.id, t.id);
    assert_eq!(1, first.clone().self_and_descendants().count());

    let (firstsplit, a) = add_leaf(first.clone(), |_| "a", Dir::H);
    assert_eq!(firstsplit.id, a.clone().root().id);
    assert_eq!(firstsplit.id, firstsplit.clone().root().id);
    assert_ne!(firstsplit.id, a.id);
    assert!(match &*firstsplit.n.borrow() {
        N::Split(_) => true,
        _ => false,
    });
    assert_eq!(vec![firstsplit.id, first.id, a.id], firstsplit.clone().self_and_descendants().map(|x| x.id).collect::<Vec<NodeId>>());

    let (secondsplit, b) = add_leaf(firstsplit.clone(), |_| "b", Dir::H);
    assert_eq!(secondsplit.id, a.clone().root().id);
    assert_eq!(vec![secondsplit.id, firstsplit.id, first.id, a.id, b.id], secondsplit.clone().self_and_descendants().map(|x| x.id).collect::<Vec<NodeId>>());

    let (thirdsplit, c) = add_leaf(b.clone(), |_| "c", Dir::H);
    assert_eq!(secondsplit.id, a.clone().root().id);
    assert_eq!(vec![secondsplit.id, firstsplit.id, first.id, a.id, thirdsplit.id, b.id, c.id], secondsplit.clone().self_and_descendants().map(|x| x.id).collect::<Vec<NodeId>>());
}

#[test]
fn test_remove() {
    let r = create_root::<&'static str>();
    let (first, _t1) = add_leaf(r.clone(), |_| "first", Dir::V);
    let (firstsplit, a) = add_leaf(first.clone(), |_| "a", Dir::H);
    let (_t2, b) = add_leaf(firstsplit.clone(), |_| "b", Dir::H);

    let root1 = a.clone().root();

    remove_from_tree(a).unwrap();

    assert_eq!(b.clone().root().id, root1.id);
    assert_eq!(vec![root1.id, first.id, b.id], root1.self_and_descendants().map(|x| x.id).collect::<Vec<NodeId>>());

    remove_from_tree(first).unwrap();

    assert_eq!(b.clone().root().id, b.id);

    assert!(b.as_content().is_some());

    remove_from_tree(b.clone()).unwrap();

    assert!(b.as_content().is_none());
}
