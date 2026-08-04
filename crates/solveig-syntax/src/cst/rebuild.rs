//! Collapses CST wrapper nodes after event construction.
//! The parser owns structure; this pass only removes mechanically empty nodes.

use la_arena::Arena;
use solveig_base::ice;

use super::{CstId, NodeOrToken, PosToken, SyntaxNode, SyntaxTree, TokenId, token_is_meaningful};

pub(super) fn collapse_trivial_wrappers(tree: &mut SyntaxTree) {
    let old_nodes = std::mem::replace(&mut tree.nodes, Arena::new());
    let old_root = tree.root;
    let mut rebuilder = CstRebuilder {
        old_nodes,
        tokens: &tree.tokens,
        nodes: Arena::new(),
        token_parents: vec![None; tree.tokens.len()],
    };

    let rebuilt = rebuilder.rebuild_node(old_root, true);
    let [NodeOrToken::Node(root)] = rebuilt.as_slice() else {
        ice!("CST root rebuild must produce exactly one root node");
    };

    tree.root = *root;
    tree.nodes = rebuilder.nodes;
    tree.token_parents = rebuilder.token_parents;

    for (index, parent) in tree.token_parents.iter().enumerate() {
        assert!(
            parent.is_some(),
            "ICE: CST collapse lost parent for token {index}"
        );
    }
}

struct CstRebuilder<'a> {
    old_nodes: Arena<SyntaxNode>,
    tokens: &'a [PosToken],
    nodes: Arena<SyntaxNode>,
    token_parents: Vec<Option<CstId>>,
}

impl CstRebuilder<'_> {
    fn rebuild_node(&mut self, old_id: CstId, force_keep: bool) -> Vec<NodeOrToken> {
        if !force_keep && self.should_collapse(old_id) {
            return self.rebuild_children(old_id, None);
        }

        let old = &self.old_nodes[old_id.0];
        let new_id = CstId(self.nodes.alloc(SyntaxNode {
            kind: old.kind,
            span: old.span,
            parent: None,
            children: Vec::new(),
            diagnostics: old.diagnostics.clone(),
        }));
        let children = self.rebuild_children(old_id, Some(new_id));
        self.nodes[new_id.0].children = children;

        vec![NodeOrToken::Node(new_id)]
    }

    fn rebuild_children(&mut self, old_id: CstId, parent: Option<CstId>) -> Vec<NodeOrToken> {
        let old_children = self.old_nodes[old_id.0].children.clone();
        let mut children = Vec::new();

        for child in old_children {
            match child {
                NodeOrToken::Node(old_child) => {
                    let rebuilt = self.rebuild_node(old_child, false);
                    for entry in rebuilt {
                        self.attach_rebuilt_child(parent, entry);
                        children.push(entry);
                    }
                }
                NodeOrToken::Token(token) => {
                    self.attach_rebuilt_child(parent, NodeOrToken::Token(token));
                    children.push(NodeOrToken::Token(token));
                }
            }
        }

        children
    }

    fn attach_rebuilt_child(&mut self, parent: Option<CstId>, child: NodeOrToken) {
        let Some(parent) = parent else {
            return;
        };

        match child {
            NodeOrToken::Node(id) => self.nodes[id.0].parent = Some(parent),
            NodeOrToken::Token(id) => {
                *self
                    .token_parents
                    .get_mut(id.index())
                    .expect("ICE: rebuilt CST token parent index is outside token stream") =
                    Some(parent);
            }
        }
    }

    fn should_collapse(&self, old_id: CstId) -> bool {
        let old = &self.old_nodes[old_id.0];

        if !old.diagnostics.is_empty() {
            return false;
        }

        let mut child_nodes = 0usize;
        for child in &old.children {
            match child {
                NodeOrToken::Node(_) => child_nodes += 1,
                NodeOrToken::Token(id) if token_is_meaningful(self.token(*id)) => {
                    return false;
                }
                NodeOrToken::Token(_) => {}
            }
        }

        child_nodes == 1
    }

    fn token(&self, id: TokenId) -> &PosToken {
        self.tokens
            .get(id.index())
            .expect("ICE: CST wrapper rebuild token id is outside token stream")
    }
}
