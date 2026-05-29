//! Control-flow graph node flags and structures.

use crate::ids::NodeId;

bitflags::bitflags! {
    /// Classifies a `FlowNode` in the binder's control-flow graph.
    ///
    /// Mirrors Go `FlowFlags` (a `uint32` `iota` enum).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::flow::FlowFlags;
    /// assert_eq!(FlowFlags::LABEL, FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL);
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/flow.go:FlowFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct FlowFlags: u32 {
        /// Unreachable code.
        const UNREACHABLE = 1 << 0;
        /// Start of the flow graph.
        const START = 1 << 1;
        /// Non-looping junction.
        const BRANCH_LABEL = 1 << 2;
        /// Looping junction.
        const LOOP_LABEL = 1 << 3;
        /// Assignment.
        const ASSIGNMENT = 1 << 4;
        /// Condition known to be true.
        const TRUE_CONDITION = 1 << 5;
        /// Condition known to be false.
        const FALSE_CONDITION = 1 << 6;
        /// Switch statement clause.
        const SWITCH_CLAUSE = 1 << 7;
        /// Potential array mutation.
        const ARRAY_MUTATION = 1 << 8;
        /// Potential assertion call.
        const CALL = 1 << 9;
        /// Temporarily reduce antecedents of a label.
        const REDUCE_LABEL = 1 << 10;
        /// Referenced as an antecedent once.
        const REFERENCED = 1 << 11;
        /// Referenced as an antecedent more than once.
        const SHARED = 1 << 12;

        /// Either label kind.
        const LABEL = Self::BRANCH_LABEL.bits() | Self::LOOP_LABEL.bits();
        /// Either condition kind.
        const CONDITION = Self::TRUE_CONDITION.bits() | Self::FALSE_CONDITION.bits();
    }
}

/// A typed index into a flow-node arena, replacing Go's `*FlowNode`.
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FlowNodeId(pub u32);

/// A typed index into a flow-list arena, replacing Go's `*FlowList`.
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FlowListId(pub u32);

/// A node in the binder's control-flow graph.
///
/// The Go original uses raw pointers for `Antecedent`/`Antecedents`; here those
/// become arena indices (see crate-level ownership notes).
///
/// Side effects: none (pure value type).
// Go: internal/ast/flow.go:FlowNode
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FlowNode {
    /// Flags classifying this flow node.
    pub flags: FlowFlags,
    /// Associated AST node, if any.
    pub node: Option<NodeId>,
    /// Antecedent for all but flow labels.
    pub antecedent: Option<FlowNodeId>,
    /// Linked list of antecedents for flow labels.
    pub antecedents: Option<FlowListId>,
}

impl Default for FlowFlags {
    fn default() -> Self {
        FlowFlags::empty()
    }
}

/// A cons cell in a linked list of flow antecedents.
///
/// Side effects: none (pure value type).
// Go: internal/ast/flow.go:FlowList
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FlowList {
    /// The flow node held by this cell.
    pub flow: Option<FlowNodeId>,
    /// The next cell, if any.
    pub next: Option<FlowListId>,
}

/// Synthetic flow data for a `switch` clause range (`FlowFlags::SWITCH_CLAUSE`).
///
/// Side effects: none (pure value type).
// Go: internal/ast/flow.go:FlowSwitchClauseData
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowSwitchClauseData {
    /// The owning `switch` statement node.
    pub switch_statement: Option<NodeId>,
    /// Start index of the case/default clause range.
    pub clause_start: i32,
    /// End index of the case/default clause range.
    pub clause_end: i32,
}

impl FlowSwitchClauseData {
    /// Reports whether the clause range is empty (`start == end`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::flow::FlowSwitchClauseData;
    /// let d = FlowSwitchClauseData { switch_statement: None, clause_start: 2, clause_end: 2 };
    /// assert!(d.is_empty());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/flow.go:FlowSwitchClauseData.IsEmpty
    pub fn is_empty(&self) -> bool {
        self.clause_start == self.clause_end
    }
}

/// Synthetic flow data for a temporary label reduction (`FlowFlags::REDUCE_LABEL`).
///
/// Side effects: none (pure value type).
// Go: internal/ast/flow.go:FlowReduceLabelData
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FlowReduceLabelData {
    /// The target label.
    pub target: Option<FlowNodeId>,
    /// The temporary antecedent list.
    pub antecedents: Option<FlowListId>,
}

#[cfg(test)]
#[path = "flow_test.rs"]
mod tests;
