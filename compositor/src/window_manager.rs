use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::iter::Iterator;
use std::pin::Pin;
use std::rc::{Rc, Weak};

use crate::wlroots_compositor::{NodeId, OutputId, Rect, View};

pub struct WindowManager {
    nodes: HashMap<NodeId, Rc<Node>>,
    mru_view: Vec<NodeId>,
    workspaces: Vec<Workspace>,
    active_workspace_index: usize,
    next_node_id: NodeId,
    dummy_workspace: Workspace,
}

pub type ViewRef<'a> = Ref<'a, Pin<Box<View>>>;

pub struct OutputInfo {
    pub id: OutputId,
    pub rect: Rect,
}

impl WindowManager {
    pub fn new() -> WindowManager {
        let ws = Workspace {
            root: Rc::new(workspace_root()),
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            output_id: None,
        };
        WindowManager {
            nodes: HashMap::new(),
            mru_view: Vec::new(),
            workspaces: vec![ws.clone()],
            active_workspace_index: 0,
            next_node_id: 1,
            dummy_workspace: ws,
        }
    }

    pub fn views_for_render<'a>(&'a self, _output: OutputId) -> impl Iterator<Item = ViewRef> {
        // TODO check output
        self.pick_views(self.mru_view.iter().map(|id| self.nodes.get(id).unwrap()))
    }

    pub fn views_for_finding<'a>(&'a self) -> impl Iterator<Item = ViewRef> {
        self.pick_views(
            self.mru_view
                .iter()
                .rev()
                .map(|id| self.nodes.get(id).unwrap()),
        )
    }

    fn pick_views<'a>(
        &'a self,
        nodes: impl Iterator<Item = &'a Rc<Node>> + 'a,
    ) -> impl Iterator<Item = ViewRef> {
        nodes.filter_map(|node| match &node.n {
            N::Leaf(leaf) => Some(leaf.view.borrow()),
            _ => None,
        })
    }

    pub fn new_node_id(&mut self) -> NodeId {
        for n in 0..10000 {
            let candidate = self.next_node_id + n;
            if candidate != 0 && !self.nodes.contains_key(&candidate) {
                self.next_node_id = candidate + 1;
                return candidate;
            }
        }

        panic!("Could not get new node id. Either compositor is bugged or you are doing something *VERY* weird.");
    }

    pub fn touch_node(&mut self, id: NodeId) {
        self.remove_from_mru(id);
        self.mru_view.push(id);
    }

    fn remove_from_mru(&mut self, id: NodeId) {
        let mru_idx = self
            .mru_view
            .iter()
            .rposition(|x| *x == id)
            .expect("BUG: view missing from mru vec");
        self.mru_view.remove(mru_idx);
    }

    pub fn add_node(&mut self, view: Pin<Box<View>>) {
        let new_root_id = self.new_node_id();
        let view_id = view.id;

        let new_leaf = Rc::new(Node {
            id: view.id,
            parent: RefCell::new(None),
            n: N::Leaf(Leaf {
                view: RefCell::new(view),
            }),
        });

        self.nodes.insert(new_leaf.id, new_leaf.clone());

        let ws = &mut self.workspaces[self.active_workspace_index];
        let workspace_top = {
            let mut container = ws.root_container();

            if container.is_empty() {
                container.push(Rc::downgrade(&new_leaf));
                new_leaf.clone()
            } else {
                let old_root = container[0].upgrade().unwrap();
                let new_root = create_split(old_root, new_leaf.clone(), new_root_id, Dir::V);
                self.nodes.insert(new_root.id, new_root.clone());
                container[0] = Rc::downgrade(&new_root);
                new_root
            }
        };
        *workspace_top.parent.borrow_mut() = Some(Rc::downgrade(&ws.root));

        self.mru_view.push(view_id);

        // TODO only configure changed views
        self.configure_views();
    }

    pub fn remove_node(&mut self, id: NodeId) {
        let node = self.nodes.get(&id).unwrap();
        let also_remove = remove_from_tree(node.clone());
        self.nodes.remove(&id);
        debug!("Remove {}", id);
        if let Some(n) = also_remove {
            self.nodes.remove(&n.id);
            debug!("Remove {}", n.id);
        }
        self.remove_from_mru(id);

        // TODO only configure changed views
        self.configure_views();
    }

    fn configure_views(&mut self) {
        debug!("Start configure");
        // TODO handle workspaces
        let ws = &mut self.workspaces[0];
        let rect = ws.rect.clone();
        for node in ws.root_container().iter() {
            configure_views(node.upgrade().unwrap(), rect.clone());
        }
        debug!("End configure");
    }

    pub fn update_outputs(&mut self, outputs: impl Iterator<Item = OutputInfo>) {
        // Just one workspace per output for now, to get started
        let mut old_workspaces = Vec::new();
        std::mem::swap(&mut old_workspaces, &mut self.workspaces);
        let mut old_workspaces: HashMap<Option<OutputId>, Workspace> = old_workspaces
            .into_iter()
            .map(|x| (x.output_id, x))
            .collect();

        self.workspaces = outputs
            .map(|o| {
                old_workspaces
                    .remove(&Some(o.id))
                    .unwrap_or_else(|| Workspace {
                        root: Rc::new(workspace_root()),
                        rect: o.rect,
                        output_id: Some(o.id),
                    })
            })
            .chain(std::iter::once(self.dummy_workspace.clone()))
            .collect();
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: Rc<Node>,
    rect: Rect,
    output_id: Option<OutputId>,
}

impl Workspace {
    fn root_container<'a>(&'a mut self) -> RefMut<'a, Vec<Weak<Node>>> {
        if let N::List(v) = &self.root.n {
            v.borrow_mut()
        } else {
            invalid_tree_op()
        }
    }
}

#[derive(Debug)]
enum N {
    Leaf(Leaf),
    Split(RefCell<Split>),
    List(RefCell<Vec<Weak<Node>>>),
}

#[derive(Debug)]
struct Node {
    id: NodeId,
    parent: RefCell<Option<Weak<Node>>>,
    n: N,
}

impl Node {
    fn parent(&self) -> Option<Rc<Node>> {
        self.parent.borrow().as_ref().map(|x| x.upgrade().unwrap())
    }
}

struct Leaf {
    view: RefCell<Pin<Box<View>>>,
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
    a: Weak<Node>,
    b: Weak<Node>,
}

impl Split {
    fn a(&self) -> Rc<Node> {
        self.a.upgrade().unwrap()
    }
    fn b(&self) -> Rc<Node> {
        self.b.upgrade().unwrap()
    }
}

fn create_split(existing_node: Rc<Node>, new_node: Rc<Node>, new_id: NodeId, dir: Dir) -> Rc<Node> {
    let new_parent = Rc::new(Node {
        id: new_id,
        parent: existing_node.parent.clone(),
        n: N::Split(RefCell::new(Split {
            dir,
            a: Rc::downgrade(&new_node),
            b: Rc::downgrade(&existing_node),
        })),
    });
    *new_node.parent.borrow_mut() = Some(Rc::downgrade(&new_parent));
    *existing_node.parent.borrow_mut() = Some(Rc::downgrade(&new_parent));

    new_parent
}

fn remove_from_tree(node: Rc<Node>) -> Option<Rc<Node>> {
    match node.n {
        N::Leaf(_) => (),
        _ => panic!("Only leaf can be removed for now..."),
    }

    let remove_parent = node.parent();
    if let Some(ref parent) = remove_parent {
        match &parent.n {
            N::Leaf(_) => invalid_tree_op(),
            N::Split(split) => {
                let sibling = sibling(&split.borrow(), &node);
                replace_node(parent, &sibling);
            }
            N::List(list) => {
                debug!("Remove from list: {}", list.borrow().len());
                list.borrow_mut()
                    .retain(|x| x.upgrade().unwrap().id != node.id);
                debug!("--> {}", list.borrow().len());
            }
        }
    }

    remove_parent
}

fn replace_node(from: &Rc<Node>, to: &Rc<Node>) {
    let parent = from.parent();
    if let Some(ref parent) = parent {
        replace_child(&parent, from, to);
    }

    *to.parent.borrow_mut() = parent.map(|x| Rc::downgrade(&x));
}

fn replace_child(parent: &Rc<Node>, from: &Rc<Node>, to: &Rc<Node>) {
    match &parent.n {
        N::Split(s) => {
            let mut s = s.borrow_mut();
            if s.a().id == from.id {
                s.a = Rc::downgrade(to);
            } else if s.b().id == from.id {
                s.b = Rc::downgrade(to);
            } else {
                panic!("Invalid child replace");
            }
        }
        N::List(l) => {
            let mut l = l.borrow_mut();
            if let Some(idx) = l.iter().position(|x| x.upgrade().unwrap().id == from.id) {
                l[idx] = Rc::downgrade(to);
            } else {
                invalid_tree_op();
            }
        }
        N::Leaf(_) => invalid_tree_op(),
    }
}

fn sibling(s: &Split, child: &Node) -> Rc<Node> {
    if s.a().id == child.id {
        s.b()
    } else if s.b().id == child.id {
        s.a()
    } else {
        invalid_tree_op();
    }
}

fn configure_views(root: Rc<Node>, rect: Rect) {
    debug!("Configure {}", root.id);
    match &root.n {
        N::Split(s) => {
            let s = s.borrow();
            match s.dir {
                Dir::H => {
                    configure_views(
                        s.a(),
                        Rect {
                            h: rect.h / 2.0,
                            ..rect
                        },
                    );
                    configure_views(
                        s.b(),
                        Rect {
                            h: rect.h / 2.0,
                            y: rect.y + rect.h / 2.0,
                            ..rect
                        },
                    );
                }
                Dir::V => {
                    configure_views(
                        s.a(),
                        Rect {
                            w: rect.w / 2.0,
                            ..rect
                        },
                    );
                    configure_views(
                        s.b(),
                        Rect {
                            w: rect.w / 2.0,
                            x: rect.x + rect.w / 2.0,
                            ..rect
                        },
                    );
                }
            }
        }
        N::List(l) => {
            for x in l.borrow().iter() {
                configure_views(x.upgrade().unwrap(), rect.clone());
            }
        }
        N::Leaf(leaf) => {
            debug!("Configure {} to {:?}", root.id, rect);
            leaf.view.borrow_mut().as_mut().configure_rect(&rect);
        }
    }
}

fn invalid_tree_op() -> ! {
    panic!("Tree state is invalid")
}

fn workspace_root() -> Node {
    Node {
        id: 0,
        parent: RefCell::new(None),
        n: N::List(RefCell::new(Vec::new())),
    }
}
