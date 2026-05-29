//! Deep cloning of subtrees (`deep_clone_node`, `deep_clone_reparse`).

use crate::visitor::VisitOptions;
use crate::{NodeArena, NodeFlags, NodeId};
use tsgo_core::text::TextRange;

impl NodeArena {
    /// Recursively deep-clones the subtree rooted at `id`.
    ///
    /// Mirrors Go's deep-clone visitor: interior nodes are rebuilt via
    /// `visit_each_child` (with lists always cloned), and leaf nodes are
    /// force-cloned. When `synthetic_location` is set, every produced node gets
    /// the synthetic range `(-1, -1)` and trailing-comma list tails get
    /// `(-2, -2)`.
    ///
    /// Side effects: pushes new nodes.
    // Go: internal/ast/deepclone.go:getDeepCloneVisitor
    fn deep_clone_visit(&mut self, id: NodeId, synthetic: bool) -> NodeId {
        let opts = VisitOptions {
            synthetic_location: synthetic,
            clone_lists: true,
        };
        let visited = self.visit_each_child(id, opts, &mut |arena, child| {
            arena.deep_clone_visit(child, synthetic)
        });
        if visited != id {
            if synthetic {
                self.set_loc(visited, TextRange::new(-1, -1));
            }
            visited
        } else {
            let cloned = self.clone_node(id);
            if synthetic {
                self.set_loc(cloned, TextRange::new(-1, -1));
            }
            cloned
        }
    }

    /// Deep-clones the subtree rooted at `id`, marking every node's location as
    /// synthetic. Used to copy a subtree across files.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::NodeArena;
    /// let mut arena = NodeArena::new();
    /// let a = arena.new_identifier("a");
    /// let clone = arena.deep_clone_node(a);
    /// assert_ne!(a, clone);
    /// ```
    ///
    /// Side effects: pushes new nodes.
    // Go: internal/ast/deepclone.go:DeepCloneNode
    pub fn deep_clone_node(&mut self, id: NodeId) -> NodeId {
        self.deep_clone_visit(id, true)
    }

    /// Deep-clones the subtree rooted at `id` for reparse: keeps real locations,
    /// rewires parent pointers, and marks the root `REPARSED`.
    ///
    /// Side effects: pushes new nodes; sets parents; sets a flag on the root.
    // Go: internal/ast/deepclone.go:DeepCloneReparse
    pub fn deep_clone_reparse(&mut self, id: NodeId) -> NodeId {
        let cloned = self.deep_clone_visit(id, false);
        self.set_parent_in_children(cloned);
        self.add_flags(cloned, NodeFlags::REPARSED);
        cloned
    }
}

#[cfg(test)]
#[path = "deepclone_test.rs"]
mod tests;
