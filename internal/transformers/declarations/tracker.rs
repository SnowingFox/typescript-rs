//! Port of Go `internal/transformers/declarations/tracker.go`: the declaration
//! emit *accessibility tracker*, Go's `SymbolTrackerImpl` + its shared
//! `diagnostics` sink.
//!
//! # Scope (round D-F3, reachable subset)
//!
//! Go's `SymbolTrackerImpl` is the [`checker.SymbolTracker`] the node builder
//! and the declaration transform feed accessibility / serialization failures
//! into; it owns a `SymbolTrackerSharedState` whose `diagnostics` slice
//! accumulates the produced [`Diagnostic`]s. This port models the reachable
//! pieces: a shared diagnostics sink ([`SymbolTracker`]) the transform appends
//! to, and [`create_diagnostic_for_node`] (Go's `createDiagnosticForNode` ->
//! `checker.NewDiagnosticForNode`). The driving accessibility checks live in the
//! transform (which owns the resolver context); the tracker is the sink the
//! diagnostics flow into.
//!
//! # Deferred (with blocked-by)
//!
//! - The node-builder tracker callbacks (`ReportInaccessibleThisError`,
//!   `ReportInaccessibleUniqueSymbolError`, `ReportPrivateInBaseOfClassExpression`,
//!   `ReportCyclicStructureError`, `ReportNonSerializableProperty`,
//!   `ReportNonlocalAugmentation`, `ReportTruncationError`, `TrackSymbol`) — they
//!   require the pseudo-type node builder to be instrumented with a tracker.
//!   blocked-by: `pseudotypenodebuilder.go` + the node-builder `SymbolTracker`
//!   handshake.
//! - `PushErrorFallbackNode`/`PopErrorFallbackNode` + `errorNameNode` fallback
//!   stack — not needed for the reachable diagnostics (the transform passes the
//!   error node directly).

use std::cell::RefCell;
use std::rc::Rc;
use tsgo_ast::{NodeArena, NodeId};
use tsgo_checker::Diagnostic;
use tsgo_diagnostics::Message;

/// The declaration-emit diagnostics sink, Go's `SymbolTrackerSharedState`'s
/// `diagnostics` slice behind the `SymbolTrackerImpl`.
///
/// Shared (`Rc<RefCell<..>>`) so the transform appends while a clone is handed
/// back to the caller (mirroring Go's `GetDiagnostics`, which reads the same
/// backing slice the tracker wrote to).
///
/// # Examples
/// ```
/// use std::cell::RefCell;
/// use std::rc::Rc;
/// use tsgo_transformers::declarations::tracker::SymbolTracker;
/// let tracker = SymbolTracker::new(Rc::new(RefCell::new(Vec::new())));
/// assert!(tracker.diagnostics().borrow().is_empty());
/// ```
///
/// Side effects: none (a handle over a shared diagnostics vector).
// Go: internal/transformers/declarations/tracker.go:SymbolTrackerImpl / SymbolTrackerSharedState
#[derive(Clone)]
pub struct SymbolTracker {
    diagnostics: Rc<RefCell<Vec<Diagnostic>>>,
}

impl SymbolTracker {
    /// Builds a tracker over the shared `diagnostics` sink.
    ///
    /// # Examples
    /// ```
    /// use std::cell::RefCell;
    /// use std::rc::Rc;
    /// use tsgo_transformers::declarations::tracker::SymbolTracker;
    /// let _tracker = SymbolTracker::new(Rc::new(RefCell::new(Vec::new())));
    /// ```
    ///
    /// Side effects: none (stores the handle).
    // Go: internal/transformers/declarations/tracker.go:NewSymbolTracker
    pub fn new(diagnostics: Rc<RefCell<Vec<Diagnostic>>>) -> SymbolTracker {
        SymbolTracker { diagnostics }
    }

    /// Records `diag` on the shared sink (Go's `SymbolTrackerSharedState.addDiagnostic`).
    ///
    /// # Examples
    /// ```
    /// use std::cell::RefCell;
    /// use std::rc::Rc;
    /// use tsgo_transformers::declarations::tracker::SymbolTracker;
    /// use tsgo_checker::{Category, Diagnostic};
    /// let tracker = SymbolTracker::new(Rc::new(RefCell::new(Vec::new())));
    /// tracker.add_diagnostic(Diagnostic {
    ///     code: 4025,
    ///     category: Category::Error,
    ///     message: "Exported variable 'b' has or is using private name 'a'.".to_string(),
    ///     start: 0,
    ///     length: 1,
    ///     related_information: Vec::new(),
    ///     message_chain: Vec::new(),
    /// });
    /// assert_eq!(tracker.diagnostics().borrow().len(), 1);
    /// ```
    ///
    /// Side effects: pushes `diag` onto the shared sink.
    // Go: internal/transformers/declarations/tracker.go:SymbolTrackerSharedState.addDiagnostic
    pub fn add_diagnostic(&self, diag: Diagnostic) {
        self.diagnostics.borrow_mut().push(diag);
    }

    /// Returns a clone of the shared diagnostics handle.
    ///
    /// # Examples
    /// ```
    /// use std::cell::RefCell;
    /// use std::rc::Rc;
    /// use tsgo_transformers::declarations::tracker::SymbolTracker;
    /// let tracker = SymbolTracker::new(Rc::new(RefCell::new(Vec::new())));
    /// let _handle = tracker.diagnostics();
    /// ```
    ///
    /// Side effects: none (clones an `Rc`).
    pub fn diagnostics(&self) -> Rc<RefCell<Vec<Diagnostic>>> {
        Rc::clone(&self.diagnostics)
    }
}

/// Builds a [`Diagnostic`] at `node` from `message` with `args` substituted
/// into its placeholders (Go's `createDiagnosticForNode` ->
/// `checker.NewDiagnosticForNode`). Mirrors the checker's `diagnostic_for_node`:
/// the span is `node`'s full text range and the related-information list starts
/// empty.
///
/// # Examples
/// ```
/// use tsgo_ast::NodeArena;
/// use tsgo_transformers::declarations::tracker::create_diagnostic_for_node;
/// let mut a = NodeArena::new();
/// let id = a.new_identifier("a");
/// let diag = create_diagnostic_for_node(
///     &a,
///     id,
///     &tsgo_diagnostics::EXPORTED_VARIABLE_0_HAS_OR_IS_USING_PRIVATE_NAME_1,
///     &["b", "a"],
/// );
/// assert_eq!(diag.code, 4025);
/// assert_eq!(diag.message, "Exported variable 'b' has or is using private name 'a'.");
/// ```
///
/// Side effects: none (reads `arena`).
// Go: internal/transformers/declarations/tracker.go:createDiagnosticForNode
pub fn create_diagnostic_for_node(
    arena: &NodeArena,
    node: NodeId,
    message: &'static Message,
    args: &[&str],
) -> Diagnostic {
    let loc = arena.loc(node);
    Diagnostic {
        code: message.code(),
        category: message.category(),
        message: tsgo_diagnostics::format(&message.to_string(), args),
        start: loc.pos(),
        length: loc.end() - loc.pos(),
        related_information: Vec::new(),
        message_chain: Vec::new(),
    }
}
