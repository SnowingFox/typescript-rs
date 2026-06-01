//! Port of the reachable subset of Go `internal/ls/change/trackerimpl.go`.
//!
//! The trivia-aware range adjustment (`get_adjusted_range` /
//! `get_adjusted_start_position` / `get_adjusted_end_position` /
//! `get_end_position_of_multiline_trailing_comment`) and the edit finalization
//! (`get_text_changes_from_changes`) that sorts edits and verifies they do not
//! overlap.
//!
//! The formatter-dependent text computation (`computeNewText` /
//! `getFormattedTextOfNode` / `getNonformattedText`) is deferred (see crate
//! root): every accumulated edit already carries its literal replacement text.

use indexmap::IndexMap;
use tsgo_ast::{Kind, NodeId};
use tsgo_astnav::{get_start_of_node, RcSourceFile};
use tsgo_core::compute_ecma_line_starts;
use tsgo_core::text::{TextPos, TextRange};
use tsgo_core::textchange::TextChange;
use tsgo_scanner::{
    compute_line_of_position, get_leading_comment_ranges, get_trailing_comment_ranges,
    skip_trivia_ex, SkipTriviaOptions,
};
use tsgo_stringutil::is_line_break;

use crate::tracker::{ChangeTracker, LeadingTriviaOption, TrailingTriviaOption};

/// Returns the byte offset at which the line containing `position` begins.
///
/// Reimplements Go's `format.GetLineStartPositionForPosition` (private in the
/// `tsgo_format` port) over a precomputed line-start slice.
///
/// Side effects: none (pure).
// Go: internal/format/util.go:GetLineStartPositionForPosition
fn get_line_start_position_for_position(line_starts: &[TextPos], position: i32) -> i32 {
    let line = compute_line_of_position(line_starts, position);
    line_starts[line as usize].0
}

impl ChangeTracker {
    /// Sorts each file's edits by start (then end) and verifies they do not
    /// overlap, then maps them to [`TextChange`]s.
    ///
    /// # Panics
    /// Panics if two edits in the same file overlap (mirrors Go's assertion).
    ///
    /// Side effects: none beyond allocating the result (reads accumulated edits).
    // Go: internal/ls/change/trackerimpl.go:getTextChangesFromChanges
    pub(crate) fn get_text_changes_from_changes(&self) -> IndexMap<String, Vec<TextChange>> {
        let mut result: IndexMap<String, Vec<TextChange>> = IndexMap::new();
        for (file_name, edits) in &self.changes {
            // Order changes by start position; on a tie, the shorter range
            // first, since an empty range (x, x) may precede (x, y) but not
            // vice-versa.
            let mut sorted: Vec<&crate::tracker::TrackerEdit> = edits.iter().collect();
            sorted.sort_by(|a, b| {
                a.range
                    .pos()
                    .cmp(&b.range.pos())
                    .then_with(|| a.range.end().cmp(&b.range.end()))
            });
            // Verify that change intervals do not overlap, except possibly at
            // end points.
            for w in sorted.windows(2) {
                if w[0].range.end() > w[1].range.pos() {
                    panic!("changes overlap: {:?} and {:?}", w[0].range, w[1].range);
                }
            }
            let text_changes: Vec<TextChange> = sorted
                .iter()
                .map(|e| TextChange::new(e.range, e.new_text.clone()))
                .collect();
            if !text_changes.is_empty() {
                result.insert(file_name.clone(), text_changes);
            }
        }
        result
    }

    /// Computes the trivia-adjusted byte range spanning `start_node` to
    /// `end_node`, per the leading/trailing trivia options.
    ///
    /// Mirrors Go's `GetAdjustedRange`, but returns a byte-offset [`TextRange`]
    /// (Go converts it to an `lsproto.Range`; see crate root).
    ///
    /// Side effects: none (pure; reads the nav context).
    // Go: internal/ls/change/trackerimpl.go:GetAdjustedRange
    pub(crate) fn get_adjusted_range(
        &self,
        source_file: &RcSourceFile,
        start_node: NodeId,
        end_node: NodeId,
        leading_option: LeadingTriviaOption,
        trailing_option: TrailingTriviaOption,
    ) -> TextRange {
        TextRange::new(
            self.get_adjusted_start_position(source_file, start_node, leading_option, false),
            self.get_adjusted_end_position(source_file, end_node, trailing_option),
        )
    }

    /// Computes the adjusted start byte offset of `node` for the given leading
    /// trivia option.
    ///
    /// Mirrors Go's `getAdjustedStartPosition`. The `LeadingTriviaOption::JSDoc`
    /// branch is inert (no reparsed JSDoc; see crate root).
    ///
    /// Side effects: none (pure; reads the nav context).
    // Go: internal/ls/change/trackerimpl.go:getAdjustedStartPosition
    pub(crate) fn get_adjusted_start_position(
        &self,
        source_file: &RcSourceFile,
        node: NodeId,
        leading_option: LeadingTriviaOption,
        has_trailing_comment: bool,
    ) -> i32 {
        let text = source_file.text();
        let line_starts = compute_ecma_line_starts(text);

        // DEFER(phase-3): JSDoc-leading start position. No node carries reparsed
        // JSDoc, so `GetJSDocCommentRanges` is empty and this falls through.
        // blocked-by: JSDoc reparser (tsgo_parser).

        let start = get_start_of_node(source_file, node, false);
        let start_of_line_pos = get_line_start_position_for_position(&line_starts, start);

        match leading_option {
            LeadingTriviaOption::Exclude => return start,
            LeadingTriviaOption::StartLine => {
                let node_loc = TextRange::new(source_file.pos(node), source_file.end(node));
                if node_loc.contains_inclusive(start_of_line_pos) {
                    return start_of_line_pos;
                }
                return start;
            }
            _ => {}
        }

        let full_start = source_file.pos(node);
        if full_start == start {
            return start;
        }
        let full_start_line_index = compute_line_of_position(&line_starts, full_start);
        let full_start_line_pos = line_starts[full_start_line_index as usize].0;
        if start_of_line_pos == full_start_line_pos {
            // Full start and start of the node are on the same line.
            // When replacing we keep the leading trivia; when deleting
            // (`IncludeAll`) we delete it.
            if leading_option == LeadingTriviaOption::IncludeAll {
                return full_start;
            }
            return start;
        }

        // If the node has trailing comments, use the comment end position since
        // that text has already been included.
        if has_trailing_comment {
            // Check first for leading comments: if the node is the first import we
            // want to exclude the trivia; otherwise we get the trailing comments.
            let mut comments = get_leading_comment_ranges(text, full_start);
            if comments.is_empty() {
                comments = get_trailing_comment_ranges(text, full_start);
            }
            if !comments.is_empty() {
                return skip_trivia_ex(
                    text,
                    comments[0].loc.end(),
                    Some(&SkipTriviaOptions {
                        stop_after_line_break: true,
                        stop_at_comments: true,
                        ..Default::default()
                    }),
                );
            }
        }

        // Get start position of the line following the one containing fullstart
        // (but only if fullstart isn't the very beginning of the file).
        let next_line_start = if full_start > 0 { 1 } else { 0 };
        let mut adjusted = line_starts[(full_start_line_index + next_line_start) as usize].0;
        // Skip whitespace / newlines.
        adjusted = skip_trivia_ex(
            text,
            adjusted,
            Some(&SkipTriviaOptions {
                stop_at_comments: true,
                ..Default::default()
            }),
        );
        line_starts[compute_line_of_position(&line_starts, adjusted) as usize].0
    }

    /// Computes the adjusted end byte offset of `node` for the given trailing
    /// trivia option.
    ///
    /// Mirrors Go's `getAdjustedEndPosition`.
    ///
    /// Side effects: none (pure; reads the nav context).
    // Go: internal/ls/change/trackerimpl.go:getAdjustedEndPosition
    pub(crate) fn get_adjusted_end_position(
        &self,
        source_file: &RcSourceFile,
        node: NodeId,
        trailing_option: TrailingTriviaOption,
    ) -> i32 {
        let text = source_file.text();
        let node_end = source_file.end(node);

        if trailing_option == TrailingTriviaOption::Exclude {
            return node_end;
        }
        if trailing_option == TrailingTriviaOption::ExcludeWhitespace {
            let mut comments = get_trailing_comment_ranges(text, node_end);
            comments.extend(get_leading_comment_ranges(text, node_end));
            if let Some(last) = comments.last() {
                let real_end = last.loc.end();
                if real_end != 0 {
                    return real_end;
                }
            }
            return node_end;
        }

        let multiline_end =
            self.get_end_position_of_multiline_trailing_comment(source_file, node, trailing_option);
        if multiline_end != 0 {
            return multiline_end;
        }

        let new_end = skip_trivia_ex(
            text,
            node_end,
            Some(&SkipTriviaOptions {
                stop_after_line_break: true,
                ..Default::default()
            }),
        );

        if new_end != node_end
            && (trailing_option == TrailingTriviaOption::Include
                || is_line_break(text.as_bytes()[(new_end - 1) as usize] as char))
        {
            return new_end;
        }
        node_end
    }

    /// Returns the end of a multiline trailing comment that extends onto a
    /// later line, or `0` when there is none.
    ///
    /// Mirrors Go's `getEndPositionOfMultilineTrailingComment`.
    ///
    /// Side effects: none (pure; reads the nav context).
    // Go: internal/ls/change/trackerimpl.go:getEndPositionOfMultilineTrailingComment
    fn get_end_position_of_multiline_trailing_comment(
        &self,
        source_file: &RcSourceFile,
        node: NodeId,
        trailing_option: TrailingTriviaOption,
    ) -> i32 {
        if trailing_option != TrailingTriviaOption::Include {
            return 0;
        }
        let text = source_file.text();
        let line_starts = compute_ecma_line_starts(text);
        let node_end = source_file.end(node);
        let node_end_line = compute_line_of_position(&line_starts, node_end);
        for comment in get_trailing_comment_ranges(text, node_end) {
            // A single-line comment ends the loop (its trivia is only this line);
            // comments on subsequent lines are also ignored.
            if comment.kind == Kind::SingleLineCommentTrivia
                || compute_line_of_position(&line_starts, comment.loc.pos()) > node_end_line
            {
                break;
            }
            // If the comment's end line is past the node end line, the comment
            // spans multiple lines and it is safe to return its end.
            let comment_end_line = compute_line_of_position(&line_starts, comment.loc.end());
            if comment_end_line > node_end_line {
                return skip_trivia_ex(
                    text,
                    comment.loc.end(),
                    Some(&SkipTriviaOptions {
                        stop_after_line_break: true,
                        stop_at_comments: true,
                        ..Default::default()
                    }),
                );
            }
        }
        0
    }
}

/// Reports whether `pos1` and `pos2` lie on the same source line.
///
/// Mirrors Go's `positionsAreOnSameLine`.
///
/// Side effects: none (pure).
// Go: internal/ls/change/delete.go:positionsAreOnSameLine
// Used by the deferred `endPositionToDeleteNodeInList` list path (blocked-by
// format::indent::get_containing_list) and by the trivia tests.
#[allow(dead_code)]
pub(crate) fn positions_are_on_same_line(source_file: &RcSourceFile, pos1: i32, pos2: i32) -> bool {
    let line_starts = compute_ecma_line_starts(source_file.text());
    get_line_start_position_for_position(&line_starts, pos1)
        == get_line_start_position_for_position(&line_starts, pos2)
}

#[cfg(test)]
#[path = "trackerimpl_test.rs"]
mod tests;
