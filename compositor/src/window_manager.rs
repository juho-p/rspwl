use crate::tree::{self, Node};
use std::collections::HashMap;
use std::iter::Iterator;
use std::pin::Pin;
use std::rc::Rc;

use crate::wlroots_compositor::{NodeId, OutputId, Rect, View};

pub struct WindowManager {
    view_nodes: HashMap<NodeId, Rc<Node>>,
    mru_view: Vec<NodeId>,
    workspaces: Vec<Workspace>,
    next_node_id: NodeId,
    dummy_workspace: Workspace,
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
            workspaces: vec![ws.clone()],
            next_node_id: 1,
            dummy_workspace: ws,
        }
    }

    pub fn views_for_render<'a>(
        &'a self,
        _output: OutputId,
    ) -> impl Iterator<Item = tree::ViewRef> {
        // TODO check output
        self.mru_view.iter().map(|id| {
            self.view_nodes
                .get(id)
                .expect("Node id is not in views")
                .view()
                .expect("View node is not a view")
        })
    }

    pub fn views_for_finding<'a>(&'a self) -> impl Iterator<Item = tree::ViewRef> {
        self.mru_view.iter().rev().map(|id| {
            self.view_nodes
                .get(id)
                .expect("Node id is not in views")
                .view()
                .expect("View node is not a view")
        })
    }

    pub fn new_node_id(&mut self) -> NodeId {
        self.next_node_id += 1;
        if self.next_node_id == 0xFFFFFFFF {
            // TODO this is not fine, figure out better id system
            panic!("BUG: Node id overflow");
        }
        self.next_node_id
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

    pub fn add_view(&mut self, view: Pin<Box<View>>) {
        let view_id = view.id;

        let new_leaf = tree::create_leaf(view);

        let active_node = match self.mru_view.last() {
            Some(idx) => self.view_nodes.get(idx).unwrap().clone(),
            None => self.workspaces[0].root.clone(),
        };
        tree::add_node(active_node, new_leaf.clone(), || self.new_node_id());

        self.view_nodes.insert(new_leaf.id, new_leaf);
        self.mru_view.push(view_id);

        info!("TREE IS");
        info!("{:?}", self.workspaces[0].root);

        // TODO only configure changed views
        self.configure_views();
    }

    pub fn remove_node(&mut self, id: NodeId) {
        // NOTE only view nodes are valid for this at the moment, others crash
        let node = self.view_nodes.get(&id).unwrap();

        tree::remove_from_tree(node.clone());
        self.view_nodes.remove(&id);
        debug!("Remove {}", id);
        self.remove_from_mru(id);

        // TODO only configure changed views
        self.configure_views();
    }

    fn configure_views(&mut self) {
        debug!("Start configure");
        // TODO handle workspaces
        let ws = &mut self.workspaces[0];
        let rect = ws.rect.clone();
        tree::configure_views(ws.root.clone(), rect);
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
                        root: tree::create_root(),
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
