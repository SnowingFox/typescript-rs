//! Port of the reachable subset of Go `internal/ls/change/tracker.go`.
//!
//! The [`ChangeTracker`] accumulator: the trivia-option enums, the internal
//! `TrackerEdit`/`DeletedNode` records, the edit-recording entry points
//! (`replace_range_with_text` / `insert_text` / `delete_range` / `delete_node` /
//! `delete_node_range` / `delete`), and [`ChangeTracker::get_changes`].
//!
//! The byte-offset / `TextChange` divergence and the deferred node-insertion and
//! list-deletion paths are documented at the crate root.

use std::rc::Rc;

use indexmap::IndexMap;
use tsgo_ast::{NodeData, NodeId};
use tsgo_astnav::RcSourceFile;
use tsgo_core::text::TextRange;
use tsgo_core::textchange::TextChange;

/// How much leading trivia (whitespace / comments) a node-relative edit covers.
///
/// Mirrors Go's `LeadingTriviaOption` (`tracker.go`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeadingTriviaOption {
    /// Exclude leading trivia (i.e. begin at the node's full start).
    None,
    /// Begin at the token start (after leading trivia).
    Exclude,
    /// Include all leading trivia on the node's own first line.
    IncludeAll,
    /// Begin at the start of the node's leading JSDoc comment, if any.
    ///
    /// Inert in this port: the parser has not reparsed JSDoc, so no node carries
    /// JSDoc and this behaves like [`LeadingTriviaOption::Exclude`].
    JSDoc,
    /// Begin at the start of the node's own line when the node spans it.
    StartLine,
}

/// How much trailing trivia a node-relative edit covers.
///
/// Mirrors Go's `TrailingTriviaOption` (`tracker.go`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingTriviaOption {
    /// Exclude trailing trivia (end at the node's end).
    None,
    /// Exclude trailing trivia (end at the node's end).
    Exclude,
    /// Include trailing comments but not trailing whitespace.
    ExcludeWhitespace,
    /// Include trailing whitespace up to and including the next line break.
    Include,
}

/// A single accumulated edit: replace the bytes in `range` with `new_text`.
///
/// The reachable port only ever produces text edits (Go's
/// `trackerEditKindText` / `trackerEditKindRemove`). The node-bearing kinds
/// (`ReplaceWithSingleNode` / `ReplaceWithMultipleNodes`) require the deferred
/// formatter and are not represented here.
///
/// Side effects: none (pure value type).
// Go: internal/ls/change/tracker.go:trackerEdit
#[derive(Clone, Debug)]
pub(crate) struct TrackerEdit {
    /// The byte range being replaced.
    pub(crate) range: TextRange,
    /// The replacement text.
    pub(crate) new_text: String,
}

/// A node queued for smart (trivia-aware) deletion, with the file it lives in.
///
/// Mirrors Go's `deletedNode { sourceFile, node }`. The `Rc<RcSourceFile>`
/// stands in for Go's `*ast.SourceFile` pointer so the deferred
/// [`ChangeTracker::finish_delete_declarations`] can revisit the file.
///
/// Side effects: none (pure value type).
// Go: internal/ls/change/tracker.go:deletedNode
pub(crate) struct DeletedNode {
    pub(crate) source_file: Rc<RcSourceFile>,
    pub(crate) node: NodeId,
}

/// Accumulates trivia-aware text edits over one or more source files and
/// produces a sorted, non-overlapping edit list.
///
/// Ported from Go's `Tracker`. See the crate root for the byte-offset /
/// `TextChange` divergence and the deferred node-insertion / list-deletion
/// paths.
///
/// # Examples
/// ```
/// use std::rc::Rc;
/// use tsgo_ls_change::ChangeTracker;
/// use tsgo_astnav::RcSourceFile;
/// use tsgo_parser::{parse_source_file, SourceFileParseOptions};
/// use tsgo_core::scriptkind::ScriptKind;
/// use tsgo_core::text::TextRange;
///
/// let text = "const x = 1;";
/// let r = parse_source_file(SourceFileParseOptions::default(), text, ScriptKind::Ts);
/// let sf = Rc::new(RcSourceFile::from_rc_arena(
///     Rc::new(r.arena), r.source_file, text.to_string()));
///
/// let mut t = ChangeTracker::new("\n");
/// t.replace_range_with_text(&sf, TextRange::new(6, 7), "y");
/// let changes = t.get_changes();
/// // The default parse options leave the file name empty, so it keys on "".
/// let edits = &changes[""];
/// assert_eq!(edits.len(), 1);
/// assert_eq!(edits[0].new_text, "y");
/// assert_eq!(edits[0].range, TextRange::new(6, 7));
/// ```
///
/// Side effects: the recording methods mutate the tracker; `get_changes`
/// finalizes queued deletions and returns the edits.
// Go: internal/ls/change/tracker.go:Tracker
pub struct ChangeTracker {
    /// The newline sequence used when synthesizing edits (e.g. `"\n"`).
    new_line: String,
    /// Accumulated edits, keyed by file name (Go's `MultiMap[*SourceFile, ...]`,
    /// resolved to file names at output time).
    pub(crate) changes: IndexMap<String, Vec<TrackerEdit>>,
    /// Nodes queued for smart deletion, resolved in `finish_delete_declarations`.
    deleted_nodes: Vec<DeletedNode>,
}

impl ChangeTracker {
    /// Creates an empty tracker that synthesizes edits with the `new_line`
    /// sequence.
    ///
    /// Side effects: none.
    // Go: internal/ls/change/tracker.go:NewTracker
    pub fn new(new_line: impl Into<String>) -> ChangeTracker {
        ChangeTracker {
            new_line: new_line.into(),
            changes: IndexMap::new(),
            deleted_nodes: Vec::new(),
        }
    }

    /// Returns the configured newline sequence.
    ///
    /// Side effects: none (pure).
    pub fn new_line(&self) -> &str {
        &self.new_line
    }

    /// Records a text replacement of `range` with `text` in `source_file`.
    ///
    /// Side effects: appends an edit to the tracker.
    // Go: internal/ls/change/tracker.go:ReplaceRangeWithText
    pub fn replace_range_with_text(
        &mut self,
        source_file: &RcSourceFile,
        range: TextRange,
        text: impl Into<String>,
    ) {
        let key = file_name(source_file).to_string();
        self.changes.entry(key).or_default().push(TrackerEdit {
            range,
            new_text: text.into(),
        });
    }

    /// Records an insertion of `text` at byte offset `pos` in `source_file`.
    ///
    /// Side effects: appends an edit to the tracker.
    // Go: internal/ls/change/tracker.go:InsertText
    pub fn insert_text(&mut self, source_file: &RcSourceFile, pos: i32, text: impl Into<String>) {
        self.replace_range_with_text(source_file, TextRange::new(pos, pos), text);
    }

    /// Records a deletion of the byte range `text_range` in `source_file`.
    ///
    /// Side effects: appends an empty-text edit to the tracker.
    // Go: internal/ls/change/tracker.go:DeleteRange
    pub fn delete_range(&mut self, source_file: &RcSourceFile, text_range: TextRange) {
        self.replace_range_with_text(source_file, text_range, "");
    }

    /// Deletes `node` immediately, expanding the range by the requested leading
    /// and trailing trivia.
    ///
    /// Mirrors Go's `DeleteNode`. Prefer [`ChangeTracker::delete`], which adds
    /// the smart list/declaration handling.
    ///
    /// Side effects: appends an empty-text edit to the tracker.
    // Go: internal/ls/change/tracker.go:DeleteNode
    pub fn delete_node(
        &mut self,
        source_file: &RcSourceFile,
        node: NodeId,
        leading_trivia: LeadingTriviaOption,
        trailing_trivia: TrailingTriviaOption,
    ) {
        let rng = self.get_adjusted_range(source_file, node, node, leading_trivia, trailing_trivia);
        self.replace_range_with_text(source_file, rng, "");
    }

    /// Deletes the span from `start_node` to `end_node` (inclusive) with the
    /// requested trivia options.
    ///
    /// Mirrors Go's `DeleteNodeRange`.
    ///
    /// Side effects: appends an empty-text edit to the tracker.
    // Go: internal/ls/change/tracker.go:DeleteNodeRange
    pub fn delete_node_range(
        &mut self,
        source_file: &RcSourceFile,
        start_node: NodeId,
        end_node: NodeId,
        leading_trivia: LeadingTriviaOption,
        trailing_trivia: TrailingTriviaOption,
    ) {
        let start =
            self.get_adjusted_start_position(source_file, start_node, leading_trivia, false);
        let end = self.get_adjusted_end_position(source_file, end_node, trailing_trivia);
        self.replace_range_with_text(source_file, TextRange::new(start, end), "");
    }

    /// Queues `node` for smart, trivia-aware deletion.
    ///
    /// The deletion is computed during [`ChangeTracker::get_changes`] (Go defers
    /// it to `finishDeleteDeclarations`), so the contained-node and
    /// trailing-comma fixups can see every queued node.
    ///
    /// Side effects: appends to the deletion queue.
    // Go: internal/ls/change/tracker.go:Delete
    pub fn delete(&mut self, source_file: &Rc<RcSourceFile>, node: NodeId) {
        self.deleted_nodes.push(DeletedNode {
            source_file: Rc::clone(source_file),
            node,
        });
    }

    /// Finalizes queued deletions and returns the accumulated edits per file,
    /// each list sorted by start (then end) and verified non-overlapping.
    ///
    /// Note: like Go's `GetChanges`, the tracker should be discarded afterward.
    ///
    /// # Panics
    /// Panics if two edits in the same file overlap (mirrors Go's assertion).
    ///
    /// Side effects: drains the deletion queue into edits, then reads them out.
    // Go: internal/ls/change/tracker.go:GetChanges
    pub fn get_changes(&mut self) -> IndexMap<String, Vec<TextChange>> {
        self.finish_delete_declarations();
        // The format-on-insert `finishNodesWithInsertionsAtStart` pass is
        // deferred (see crate root); no node-insertion edits exist to finalize.
        self.get_text_changes_from_changes()
    }

    /// Pushes a pre-computed `(range, text)` edit for the file named `key`.
    ///
    /// Side effects: appends an edit to the tracker.
    pub(crate) fn add_edit(&mut self, key: &str, range: TextRange, text: impl Into<String>) {
        self.changes
            .entry(key.to_string())
            .or_default()
            .push(TrackerEdit {
                range,
                new_text: text.into(),
            });
    }

    /// Drains the deletion queue, computing a trivia-aware edit for each node
    /// not contained within another deleted node.
    ///
    /// Side effects: appends edits to the tracker; empties the deletion queue.
    // Go: internal/ls/change/tracker.go:finishDeleteDeclarations
    fn finish_delete_declarations(&mut self) {
        let deleted = std::mem::take(&mut self.deleted_nodes);

        for (i, del) in deleted.iter().enumerate() {
            let sf = Rc::clone(&del.source_file);
            // Skip if this node is contained within another deleted node.
            let mut is_contained = false;
            for (j, other) in deleted.iter().enumerate() {
                if i != j
                    && Rc::ptr_eq(&other.source_file, &del.source_file)
                    && other.node != del.node
                    && range_contains_range_exclusive(&sf, other.node, del.node)
                {
                    is_contained = true;
                    break;
                }
            }
            if is_contained {
                continue;
            }

            crate::delete::delete_declaration(self, &sf, del.node);
        }

        // Go also fixes up trailing commas for last-in-list deletions here; that
        // path is gated on `format.GetContainingList`, which the `tsgo_format`
        // port stubs to `None`, so it is inert in this port.
        // DEFER(phase-7): trailing-comma fixup for deleted last-in-list nodes.
        // blocked-by: format::indent::get_containing_list.
    }
}

/// Returns the (normalized) file name of `source_file`'s root `SourceFile`.
///
/// Side effects: none (pure).
pub(crate) fn file_name(source_file: &RcSourceFile) -> &str {
    match source_file.arena().data(source_file.root()) {
        NodeData::SourceFile(d) => &d.file_name,
        _ => "",
    }
}

/// Reports whether `outer`'s range strictly contains `inner`'s range.
///
/// Side effects: none (pure).
// Go: internal/ls/change/tracker.go:rangeContainsRangeExclusive
fn range_contains_range_exclusive(sf: &RcSourceFile, outer: NodeId, inner: NodeId) -> bool {
    sf.pos(outer) < sf.pos(inner) && sf.end(inner) < sf.end(outer)
}

#[cfg(test)]
#[path = "tracker_test.rs"]
mod tests;
