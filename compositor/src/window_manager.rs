use crate::tree::{self, Direction};
use std::cell::Ref;
use std::collections::HashMap;
use std::iter::Iterator;
use std::pin::Pin;
use std::rc::Rc;

use crate::types::{NodeId, Result, Rect};
use crate::wlroots_compositor::{OutputId, View};

type Node = tree::Node<Window>;

pub struct ViewRef<'a> {
    n: Ref<'a, tree::N<Window>>,
    rect: Ref<'a, Rect>,
}

impl<'a> ViewRef<'a> {
    pub fn content_and_rect(&'a self) -> (&'a Pin<Box<View>>, &'a Rect) {
        match &*self.n {
            tree::N::Leaf(l) => (&l.content.view, &*self.rect),
            _ => panic!("ouch"),
        }
    }
}

pub struct Window {
    pub view: Pin<Box<View>>,
    pub workspace: usize,
}

pub struct Workspace {
    root: Rc<Node>,
    rect: Rect,
    output_id: Option<OutputId>,
}

pub struct WindowManager {
    view_nodes: HashMap<NodeId, Rc<Node>>,
    mru_view: Vec<NodeId>,
    workspaces: Vec<Workspace>,
}

pub struct OutputInfo {
    pub id: OutputId,
    pub rect: Rect,
}

impl WindowManager {
    pub fn new() -> WindowManager {
        let ws = Workspace {
            root: tree::create_root(),
            // TODO: just use node dimensions
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            output_id: None,
        };
        WindowManager {
            view_nodes: HashMap::new(),
            mru_view: Vec::new(),
            workspaces: vec![ws],
        }
    }

    pub fn find_view<'a>(&'a self, id: NodeId) -> Option<ViewRef<'a>> {
        self.view_nodes.get(&id)
            .map(|node| ViewRef {
                n: node.n.borrow(), // n MUST be Leaf. If not, will crash later
                rect: node.rect.borrow(),
            })
    }

    // TODO ren
    pub fn views_for_render<'a>(&'a self, _output: OutputId) -> impl Iterator<Item = ViewRef<'a>> {
        // TODO check output
        self.mru_view.iter().map(|id| {
            self.find_view(*id).expect("View not there where it should be")
        })
    }

    pub fn views_for_finding<'a>(&'a self) ->  impl Iterator<Item = ViewRef<'a>> {
        self.mru_view.iter().rev().map(|id| {
            self.find_view(*id).expect("View not there where it should be")
        })
    }

    pub fn touch_node(&mut self, id: NodeId) {
        println!("Touch {}", id);
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

    pub fn active_node(&self) -> Option<Rc<Node>> {
        self.mru_view.last()
            .and_then(|x| self.view_nodes.get(x))
            .cloned()
    }

    pub fn add_view(&mut self, create_view: impl FnOnce(NodeId) -> Pin<Box<View>>) -> NodeId {
        let active_node = self.active_node().unwrap_or_else(|| self.workspaces[0].root.clone());

        let workspace = 0;

        let (parent, new_leaf) = tree::add_leaf(
            active_node.clone(),
            |id| Window {
                view: create_view(id),
                workspace,
            },
            split_dir(active_node),
        );

        self.workspaces[workspace].root = parent.root();

        self.view_nodes.insert(new_leaf.id, new_leaf.clone());
        self.mru_view.push(new_leaf.id);
        println!("Added {}", new_leaf.id);

        // TODO only configure changed views
        self.configure_views();

        new_leaf.id
    }

    pub fn remove_node(&mut self, id: NodeId) -> Result<()> {
        let Some((workspace, node)) =
            self.view_nodes.get(&id).map(|n| (0, n)) // TODO workspace
            else { return Err(format!("No window for {}", id)); };

        self.workspaces[workspace].root = tree::remove_from_tree(node.clone())?;

        self.view_nodes.remove(&id);
        println!("Remove {}", id);
        self.remove_from_mru(id);

        // TODO only configure changed views
        self.configure_views();
        Ok(())
    }

    pub fn configure_views(&mut self) {
        println!("Start configure");
        // TODO handle workspaces
        let ws = &mut self.workspaces[0];
        let rect = ws.rect.clone();
        configure_views(ws.root.clone(), rect);
        println!("End configure");
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
                    .map(|w| Workspace { rect: o.rect.clone(), ..w })
                    .unwrap_or_else(|| Workspace {
                        root: tree::create_root(),
                        rect: o.rect,
                        output_id: Some(o.id),
                    })
            })
            .collect();
        self.configure_views();
    }

    pub fn neighbor(&self, direction: Direction) -> Option<Rc<Node>> {
        fn overlaps(a1: f32, l1: f32, a2: f32, l2: f32) -> bool {
            // TODO gaps?
            dbg!(a1, a2, l1, l2);
            a1 + l1 >= a2 && a2 + l2 >= a1 
        }

        let active = self.active_node()?;
        let active_rect = active.rect.borrow();
        let potential_neighbors = tree::nodes_to_direction(&active, direction);

        let n = self.mru_view.iter().rev()
            .filter_map(|nodeid| potential_neighbors.get(nodeid))
            .find(|node| {
                let r = node.rect.borrow();
                if direction == Direction::Up || direction == Direction::Down {
                    overlaps(r.x, r.w, active_rect.x, active_rect.w)
                } else {
                    overlaps(r.y, r.h, active_rect.y, active_rect.h)
                }
            })
            .cloned();
        dbg!(n.is_some());
        n
    }
}

fn split_dir(node: Rc<Node>) -> tree::SplitDir {
    if node.rect.borrow().h > node.rect.borrow().w {
        tree::SplitDir::H
    } else {
        tree::SplitDir::V
    }
}

fn configure_views(root: Rc<Node>, rect: Rect) {
    use tree::SplitDir;

    *root.rect.borrow_mut() = rect.clone();

    match &mut *root.n.borrow_mut() {
        tree::N::Placeholder => {}
        tree::N::Split(s) => match s.dir {
            SplitDir::H => {
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
            SplitDir::V => {
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
        },
        tree::N::Leaf(leaf) => {
            leaf.content.view.as_mut().configure_rect(&rect);
        }
    }
}
