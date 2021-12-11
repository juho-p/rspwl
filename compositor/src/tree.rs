use std::cell::{Ref, RefCell};
use std::iter::{self, Iterator};
use std::ops::Deref;
use std::pin::Pin;
use std::rc::{Rc, Weak};

use crate::wlroots_compositor::{NodeId, Rect, View};

#[derive(Debug)]
enum N {
    Root(RefCell<Option<Rc<Node>>>),
    Leaf(RefCell<Leaf>),
    Split(RefCell<Split>),
}

#[derive(Debug)]
pub struct Node {
    pub id: NodeId,
    parent: RefCell<Option<Weak<Node>>>,
    n: N,
}

pub struct ChildIter {
    node: Rc<Node>,
    n: usize,
}

impl Iterator for ChildIter {
    type Item = Rc<Node>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = match &self.node.n {
            N::Root(r) => {
                if self.n == 0 {
                    r.borrow().clone()
                } else {
                    None
                }
            }
            N::Leaf(_) => None,
            N::Split(s) => match self.n {
                0 => Some(s.borrow().a.clone()),
                1 => Some(s.borrow().b.clone()),
                _ => None,
            },
        };
        self.n += 1;
        result
    }
}

impl Node {
    fn parent(&self) -> Option<Rc<Node>> {
        self.parent.borrow().as_ref().map(|x| x.upgrade().unwrap())
    }

    pub fn children(self: Rc<Self>) -> ChildIter {
        ChildIter {
            node: self,
            n: 0,
        }
    }

    // NOTE this is not that cheap operation at the moment
    fn descendants(self: Rc<Node>) -> Box<dyn Iterator<Item = Rc<Node>>> {
        Box::new(
            self.children()
            .flat_map(|node| iter::once(node.clone()).chain(node.descendants()))
        )
    }

    pub fn view(&self) -> Option<ViewRef> {
        match &self.n {
            N::Leaf(l) => Some(ViewRef { leaf_ref: l.borrow() }),
            _ => None,
        }
    }

    pub fn views_rect(self: Rc<Self>) -> Option<Rect> {
        iter::once(self.clone())
            .chain(self.descendants())
            .filter_map(|n| n.view().map(|v| v.rect.clone()))
            .reduce(|a, b| Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                w: a.w.max(b.w),
                h: a.h.max(b.h),
            })
    }
}

pub struct ViewRef<'a> {
    leaf_ref: Ref<'a, Leaf>,
}

impl<'a> Deref for ViewRef<'a> {
    type Target = Pin<Box<View>>;
    fn deref(&self) -> &Pin<Box<View>> {
        &self.leaf_ref.view
    }
}

struct Leaf {
    view: Pin<Box<View>>,
}

impl std::fmt::Debug for Leaf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Leaf { ... }")
    }
}

#[derive(Debug)]
enum Dir {
    // TODO: make use of this
    #[allow(unused)]
    H,
    V,
}

#[derive(Debug)]
struct Split {
    dir: Dir,
    a: Rc<Node>,
    b: Rc<Node>,
}

fn invalid_tree_op() -> ! {
    panic!("Tree state is invalid")
}

pub fn create_root() -> Rc<Node> {
    Rc::new(Node {
        id: 0,
        parent: RefCell::new(None),
        n: N::Root(RefCell::new(None)),
    })
}

pub fn create_leaf(view: Pin<Box<View>>) -> Rc<Node> {
    Rc::new(Node {
        id: view.id,
        parent: RefCell::new(None),
        n: N::Leaf(RefCell::new(Leaf { view: view })),
    })
}

pub fn add_node(tree_node: Rc<Node>, new_node: Rc<Node>, mut id_gen: impl FnMut() -> NodeId) {
    match &tree_node.n {
        N::Root(r) => {
            let mut r = r.borrow_mut();
            if r.is_some() {
                panic!("Can only add node to root if it's empty");
            }
            *new_node.parent.borrow_mut() = Some(Rc::downgrade(&tree_node));
            *r = Some(new_node);
        }
        _ => {
            let dir = next_split_dir(&tree_node);
            split_from(tree_node, new_node, id_gen(), dir);
        }
    }
}

fn split_from(tree_node: Rc<Node>, new_node: Rc<Node>, split_id: NodeId, dir: Dir) {
    let parent = tree_node.parent().expect("Can't split without parent");

    let new_split = Rc::new(Node {
        id: split_id,
        parent: tree_node.parent.clone(),
        n: N::Split(RefCell::new(Split {
            dir,
            a: tree_node.clone(),
            b: new_node.clone(),
        })),
    });

    *new_node.parent.borrow_mut() = Some(Rc::downgrade(&new_split));
    *tree_node.parent.borrow_mut() = Some(Rc::downgrade(&new_split));
    replace_child(&parent, &tree_node, new_split);
}

fn next_split_dir(node: &Rc<Node>) -> Dir {
    if let Some(rect) = node.clone().views_rect() {
        if rect.h > rect.w {
            return Dir::H
        }
    }
    Dir::V
}

pub fn remove_from_tree(node: Rc<Node>) {
    match node.n {
        N::Leaf(_) => (),
        _ => panic!("Only leaf can be removed for now..."),
    }

    if let Some(ref parent) = node.parent() {
        match &parent.n {
            N::Leaf(_) => invalid_tree_op(),
            N::Split(split) => {
                let sibling = sibling(&split.borrow(), &node);
                replace_node(parent, sibling);
            }
            N::Root(r) => {
                *r.borrow_mut() = None;
            }
        }
    }
}

fn replace_node(from: &Rc<Node>, to: Rc<Node>) {
    let parent = from.parent();
    if let Some(ref parent) = parent {
        replace_child(&parent, from, to.clone());
    }

    *to.parent.borrow_mut() = parent.map(|x| Rc::downgrade(&x));
}

fn replace_child(parent: &Rc<Node>, from: &Rc<Node>, to: Rc<Node>) {
    match &parent.n {
        N::Root(r) => {
            *r.borrow_mut() = Some(to);
        }
        N::Split(s) => {
            let mut s = s.borrow_mut();
            if s.a.id == from.id {
                s.a = to;
            } else if s.b.id == from.id {
                s.b = to;
            } else {
                panic!("Invalid child replace");
            }
        }
        N::Leaf(_) => invalid_tree_op(),
    }
}

fn sibling(s: &Split, child: &Node) -> Rc<Node> {
    if s.a.id == child.id {
        s.b.clone()
    } else if s.b.id == child.id {
        s.a.clone()
    } else {
        invalid_tree_op();
    }
}

pub fn configure_views(root: Rc<Node>, rect: Rect) {
    debug!("Configure {}", root.id);
    match &root.n {
        N::Root(r) => {
            if let Some(child) = r.borrow().clone() {
                configure_views(child, rect);
            }
        }
        N::Split(s) => {
            let s = s.borrow();
            match s.dir {
                Dir::H => {
                    configure_views(
                        s.a.clone(),
                        Rect {
                            h: rect.h / 2.0,
                            ..rect
                        },
                    );
                    configure_views(
                        s.b.clone(),
                        Rect {
                            h: rect.h / 2.0,
                            y: rect.y + rect.h / 2.0,
                            ..rect
                        },
                    );
                }
                Dir::V => {
                    configure_views(
                        s.a.clone(),
                        Rect {
                            w: rect.w / 2.0,
                            ..rect
                        },
                    );
                    configure_views(
                        s.b.clone(),
                        Rect {
                            w: rect.w / 2.0,
                            x: rect.x + rect.w / 2.0,
                            ..rect
                        },
                    );
                }
            }
        }
        N::Leaf(leaf) => {
            debug!("Configure {} to {:?}", root.id, rect);
            leaf.borrow_mut().view.as_mut().configure_rect(&rect);
        }
    }
}
