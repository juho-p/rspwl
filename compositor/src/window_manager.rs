use crate::tree;
use std::collections::HashMap;
use std::iter::Iterator;
use std::pin::Pin;
use std::rc::Rc;

use crate::types::{NodeId, Result};
use crate::wlroots_compositor::{OutputId, Rect, View};

type Node = tree::Node<Window>;
pub type WindowRef<'a> = tree::LeafContentRef<'a, Window>;

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

    // TODO ren
    pub fn views_for_render<'a>(&'a self, _output: OutputId) -> impl Iterator<Item = WindowRef> {
        // TODO check output
        self.mru_view.iter().map(|id| {
            self.view_nodes
                .get(id)
                .expect("Node id is not in views")
                .as_content()
                .expect("View node is not a view")
        })
    }

    pub fn views_for_finding<'a>(&'a self) -> impl Iterator<Item = WindowRef> {
        self.mru_view.iter().rev().map(|id| {
            self.view_nodes
                .get(id)
                .expect("Node id is not in views")
                .as_content()
                .expect("View node is not a view")
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

    pub fn add_view(&mut self, create_view: impl FnOnce(NodeId) -> Pin<Box<View>>) -> NodeId {
        let active_node = match self.mru_view.last() {
            Some(idx) => self.view_nodes.get(idx).unwrap().clone(),
            None => self.workspaces[0].root.clone(),
        };

        let workspace = active_node.as_content().map(|v| v.workspace).unwrap_or(0);

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
            self.view_nodes.get(&id).and_then(|n| n.as_content().map(|w| (w.workspace, n)))
            else { return Err(format!("No window for {}", id)); };

        self.workspaces[workspace].root = tree::remove_from_tree(node.clone())?;

        self.view_nodes.remove(&id);
        println!("Remove {}", id);
        self.remove_from_mru(id);

        // TODO only configure changed views
        self.configure_views();
        Ok(())
    }

    fn configure_views(&mut self) {
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
                    .unwrap_or_else(|| Workspace {
                        root: tree::create_root(),
                        rect: o.rect,
                        output_id: Some(o.id),
                    })
            })
            .collect();
    }
}

fn views_rect(node: Rc<Node>) -> Option<Rect> {
    node.self_and_descendants()
        .filter_map(|n| n.as_content().map(|v| v.view.rect.clone()))
        .reduce(|a, b| Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: a.w.max(b.w),
            h: a.h.max(b.h),
        })
}

fn split_dir(node: Rc<Node>) -> tree::Dir {
    if let Some(rect) = views_rect(node.clone()) {
        if rect.h > rect.w {
            return tree::Dir::H;
        }
    }
    tree::Dir::V
}

fn configure_views(root: Rc<Node>, rect: Rect) {
    use tree::Dir;
    match &mut *root.n.borrow_mut() {
        tree::N::Placeholder => {}
        tree::N::Split(s) => match s.dir {
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
        },
        tree::N::Leaf(leaf) => {
            leaf.content.view.as_mut().configure_rect(&rect);
        }
    }
}
