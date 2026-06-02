//! Port of Go `internal/fourslash/fourslash.go`: the [`FourslashTest`] driver
//! plus its verify/navigation command surface.
//!
//! Given parsed [`TestData`], [`new_fourslash`] builds an in-memory
//! [`LanguageService`] over the marker-stripped files and exposes the caret
//! navigation primitive ([`FourslashTest::go_to_marker`]) plus the verify
//! commands that drive the already-ported `tsgo_ls` providers:
//! - **quick info**: [`FourslashTest::verify_quick_info_at`] /
//!   [`FourslashTest::verify_quick_info_is`] / [`FourslashTest::verify_quick_info_exists`]
//!   (drive `get_quick_info_at_position`);
//! - **completions**: [`FourslashTest::verify_completions_include`] /
//!   [`FourslashTest::verify_completions_exact`] /
//!   [`FourslashTest::verify_completions_excludes`] (drive `provide_completions`);
//! - **go-to-definition**: [`FourslashTest::verify_go_to_definition`] /
//!   [`FourslashTest::verify_definition_at`] (drive `provide_definition`);
//! - **find-all-references**: [`FourslashTest::verify_references_at`] (drives
//!   `provide_references`);
//! - **diagnostics**: [`FourslashTest::verify_number_of_errors_in_current_file`]
//!   / [`FourslashTest::verify_no_errors`] (drive the LS diagnostics).
//!
//! # Divergence from Go
//!
//! Go's `FourslashTest` drives an in-memory **LSP server** over channels
//! (`lsptestutil.NewLSPClient`, backed by `internal/lsp` + `internal/project`)
//! and "opens" files with `textDocument/didOpen` notifications. Those crates
//! are P8 and not yet ported, so this builds and drives the in-process
//! [`tsgo_ls::LanguageService`] directly — the same way the `tsgo_ls` feature
//! tests construct a service over an in-memory program. Consequences:
//! - `go_to_marker` only moves the caret state; "opening" a file is a no-op
//!   because the program already holds every file (Go sends `didOpen`).
//! - The quick-info commands compare the reachable type string (Go compares the
//!   full markdown hover body, which `tsgo_ls` has not yet ported — see its
//!   `hover` module).
//! - The go-to-definition and find-all-references commands compare against the
//!   resolved positions / the test's `[|ranges|]` directly, because Go's
//!   `VerifyBaseline*` machinery (`baselineutil.go`) is deferred. The
//!   semantics (how the marker resolves, drives the LS, and is compared) match
//!   Go's; only the baseline-recording transport differs.
//! - Symlinks and dynamic (`untitled:`) files are not added to the VFS this
//!   round (no smoke case needs them).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use tsgo_compiler::{new_compiler_host, new_program, ProgramOptions};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_ls::{CompletionList, LanguageService, LanguageServiceHost, QuickInfo};
use tsgo_ls_lsconv::file_name_to_document_uri;
use tsgo_lsproto::{Diagnostic, DiagnosticSeverity, Location, Position};
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::{to_path, ComparePathsOptions};
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::test_parser::{
    parse_test_data, FourslashError, Marker, MarkerOrRange, RangeMarker, TestData,
};

/// The root directory the in-memory program is hosted under (Go: `rootDir`).
// Go: internal/fourslash/fourslash.go:rootDir
const ROOT_DIR: &str = "/";

/// The default base file name for the implicit first file (Go derives this from
/// the test function name via `getBaseFileNameFromTest`; this foundation uses a
/// fixed name since Rust `#[test]`s pass content directly).
const DEFAULT_FILE_NAME: &str = "mainFile.ts";

/// The in-memory [`LanguageServiceHost`]: answers file reads and case
/// sensitivity from an owned snapshot (the foundation analogue of Go's project
/// layer / LSP server state).
///
/// Side effects: none (owns the snapshot).
// Go: internal/fourslash/fourslash.go:FourslashTest (the server's host state)
struct FourslashHost {
    files: HashMap<String, String>,
    case_sensitive: bool,
}

impl LanguageServiceHost for FourslashHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.files.get(file_name).cloned()
    }
}

/// The fourslash test driver: parsed [`TestData`] plus the language service it
/// drives, and the current caret state (active file + position + last marker).
///
/// Side effects: holds the in-process language service (which owns the compiler
/// program).
// Go: internal/fourslash/fourslash.go:FourslashTest
pub struct FourslashTest {
    test_data: TestData,
    ls: LanguageService,
    active_filename: String,
    current_caret_position: Position,
    last_known_marker_name: Option<String>,
}

/// Builds a [`FourslashTest`] from fourslash `content`, parsing the markup and
/// constructing an in-memory [`LanguageService`] over the resulting files.
///
/// The implicit first file (content before any `// @filename:`) is named
/// [`DEFAULT_FILE_NAME`]. The caret starts at the beginning of the first file.
///
/// # Panics
/// Panics if `content` is not valid fourslash markup (Go fails the test via
/// `t.Fatalf`); see [`try_new_fourslash`] for the non-panicking form.
///
/// Side effects: builds a compiler program (parses every file) and a language
/// service over an in-memory file system.
// Go: internal/fourslash/fourslash.go:NewFourslash
pub fn new_fourslash(content: &str) -> FourslashTest {
    try_new_fourslash(content).expect("valid fourslash markup")
}

/// The non-panicking form of [`new_fourslash`], returning a [`FourslashError`]
/// when the markup fails to parse.
///
/// Side effects: as [`new_fourslash`].
// Go: internal/fourslash/fourslash.go:NewFourslash (parse step)
pub fn try_new_fourslash(content: &str) -> Result<FourslashTest, FourslashError> {
    let file_name = DEFAULT_FILE_NAME;
    let test_data = parse_test_data(content, file_name)?;

    // The marker-stripped files (already normalized to absolute paths).
    let files: Vec<(String, String)> = test_data
        .files
        .iter()
        .map(|f| (f.file_name().to_string(), f.content.clone()))
        .collect();

    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(
        files.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        true,
    ));
    let host = Arc::new(new_compiler_host(ROOT_DIR, fs, "/lib"));
    let roots: Vec<String> = files.iter().map(|(k, _)| k.clone()).collect();
    let config = new_parsed_command_line(
        CompilerOptions::default(),
        roots,
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: ROOT_DIR.to_string(),
        },
    );
    let program = new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    });
    let ls_host: Rc<dyn LanguageServiceHost> = Rc::new(FourslashHost {
        files: files.iter().cloned().collect(),
        case_sensitive: true,
    });
    let ls = LanguageService::new(to_path(ROOT_DIR, ROOT_DIR, true), program, ls_host);

    let active_filename = test_data
        .files
        .first()
        .map(|f| f.file_name().to_string())
        .unwrap_or_default();

    Ok(FourslashTest {
        test_data,
        ls,
        active_filename,
        current_caret_position: Position {
            line: 0,
            character: 0,
        },
        last_known_marker_name: None,
    })
}

impl FourslashTest {
    /// The parsed test data this driver was built from.
    ///
    /// Side effects: none (pure).
    pub fn test_data(&self) -> &TestData {
        &self.test_data
    }

    /// All markers in the test case (named and anonymous), in source order.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/fourslash.go:FourslashTest.Markers
    pub fn markers(&self) -> &[Marker] {
        &self.test_data.markers
    }

    /// The marker with the given name, or `None`.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/fourslash.go:FourslashTest.MarkerByName
    pub fn marker_by_name(&self, name: &str) -> Option<&Marker> {
        self.test_data.marker_positions.get(name)
    }

    /// All ranges in the test case.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/fourslash.go:FourslashTest.Ranges
    pub fn ranges(&self) -> &[RangeMarker] {
        &self.test_data.ranges
    }

    /// The currently active file name.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/fourslash.go:FourslashTest (activeFilename)
    pub fn active_filename(&self) -> &str {
        &self.active_filename
    }

    /// The current caret position (LSP `(line, character)`).
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/fourslash.go:FourslashTest (currentCaretPosition)
    pub fn current_caret_position(&self) -> Position {
        self.current_caret_position.clone()
    }

    /// The name of the last marker the caret was moved to, if any.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/fourslash.go:FourslashTest (lastKnownMarkerName)
    pub fn last_known_marker_name(&self) -> Option<&str> {
        self.last_known_marker_name.as_deref()
    }

    /// Moves the caret to the named marker: makes the marker's file active and
    /// sets the caret to the marker's LSP position.
    ///
    /// Returns an error if no marker with that name exists.
    ///
    /// Side effects: updates the active file + caret + last-known marker.
    // Go: internal/fourslash/fourslash.go:FourslashTest.GoToMarker
    pub fn go_to_marker(&mut self, marker_name: &str) -> Result<(), FourslashError> {
        let (file_name, position, name) = {
            let marker = self
                .test_data
                .marker_positions
                .get(marker_name)
                .ok_or_else(|| FourslashError(format!("Marker '{marker_name}' not found")))?;
            (
                marker.file_name().to_string(),
                marker.ls_pos(),
                marker.name.clone(),
            )
        };
        self.go_to_marker_or_range(&file_name, position, name);
        Ok(())
    }

    /// The shared body of `GoToMarker`/`GoToMarkerOrRange`: switch to `file_name`
    /// and move the caret to `position`, recording `name` as the last marker.
    ///
    /// Side effects: updates the active file + caret + last-known marker.
    // Go: internal/fourslash/fourslash.go:FourslashTest.goToMarker
    fn go_to_marker_or_range(&mut self, file_name: &str, position: Position, name: Option<String>) {
        self.ensure_active_file(file_name);
        self.current_caret_position = position;
        self.last_known_marker_name = name;
    }

    /// Makes `file_name` the active file.
    ///
    /// Foundation note: Go opens the file via an LSP `didOpen`; here the program
    /// already holds every file, so this only updates the active-file state.
    ///
    /// Side effects: updates the active file.
    // Go: internal/fourslash/fourslash.go:FourslashTest.ensureActiveFile
    fn ensure_active_file(&mut self, file_name: &str) {
        if self.active_filename != file_name {
            self.active_filename = file_name.to_string();
        }
    }

    /// Moves the caret to `marker_name` and returns the language service's quick
    /// info at that position (or `None` when the token has no resolvable
    /// symbol/type).
    ///
    /// Side effects: moves the caret; binds + checks the program via the
    /// language service.
    // Go: internal/fourslash/fourslash.go:FourslashTest.getQuickInfoAtCurrentPosition
    pub fn quick_info_at(
        &mut self,
        marker_name: &str,
    ) -> Result<Option<QuickInfo>, FourslashError> {
        self.go_to_marker(marker_name)?;
        let file_name = self.active_filename.clone();
        let position = self.current_caret_position.clone();
        Ok(self.ls.get_quick_info_at_position(&file_name, position))
    }

    /// Verifies that the quick info type string at `marker_name` equals
    /// `expected_text`, returning an error otherwise (or when there is no quick
    /// info).
    ///
    /// Foundation note: Go's `VerifyQuickInfoAt` also takes an
    /// `expectedDocumentation` and compares the full markdown hover body
    /// (` ```typescript\n<text>\n```\n<doc> `); `tsgo_ls` has only ported the
    /// reachable type string, so this compares that string. The documentation
    /// and markdown formatting are deferred with the `tsgo_ls` hover surface.
    ///
    /// Side effects: as [`Self::quick_info_at`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoAt
    pub fn verify_quick_info_at(
        &mut self,
        marker_name: &str,
        expected_text: &str,
    ) -> Result<(), FourslashError> {
        let quick_info = self.quick_info_at(marker_name)?;
        let prefix = self.current_position_prefix();
        match quick_info {
            None => Err(FourslashError(format!(
                "{prefix}Expected quick info at marker '{marker_name}' but got none"
            ))),
            Some(qi) if qi.text == expected_text => Ok(()),
            Some(qi) => Err(FourslashError(format!(
                "{prefix}Quick info mismatch: expected {expected_text:?}, got {:?}",
                qi.text
            ))),
        }
    }

    /// Returns the language service's quick info at the **current** caret
    /// position (the active file + caret a prior navigation set), or `None` when
    /// the token there has no resolvable symbol/type.
    ///
    /// Unlike [`Self::quick_info_at`] (which navigates to a marker first), this
    /// reads the current position — the primitive Go's `VerifyQuickInfoIs` /
    /// `VerifyQuickInfoExists` build on (the caret is positioned by a prior
    /// `GoToMarker`).
    ///
    /// Side effects: binds + checks the program via the language service.
    // Go: internal/fourslash/fourslash.go:FourslashTest.getQuickInfoAtCurrentPosition
    pub fn quick_info_at_current_position(&mut self) -> Option<QuickInfo> {
        let file_name = self.active_filename.clone();
        let position = self.current_caret_position.clone();
        self.ls.get_quick_info_at_position(&file_name, position)
    }

    /// Verifies that the quick info type string at the **current** caret position
    /// equals `expected_text`, returning an error otherwise (or when there is no
    /// quick info there).
    ///
    /// Mirrors Go's `VerifyQuickInfoIs`, which reads quick info at the current
    /// caret (set by a prior `GoToMarker`) and compares it. Foundation note: Go
    /// compares the full markdown hover body
    /// (` ```typescript\n<text>\n```\n<doc> `); `tsgo_ls` has ported only the
    /// reachable type string, so this compares that string (the documentation
    /// and markdown formatting are deferred with the `tsgo_ls` hover surface).
    ///
    /// Side effects: as [`Self::quick_info_at_current_position`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoIs
    pub fn verify_quick_info_is(&mut self, expected_text: &str) -> Result<(), FourslashError> {
        let prefix = self.current_position_prefix();
        match self.quick_info_at_current_position() {
            None => Err(FourslashError(format!(
                "{prefix}Expected hover result but got none"
            ))),
            Some(qi) if qi.text == expected_text => Ok(()),
            Some(qi) => Err(FourslashError(format!(
                "{prefix}Quick info mismatch: expected {expected_text:?}, got {:?}",
                qi.text
            ))),
        }
    }

    /// Verifies that quick info *exists* at the **current** caret position,
    /// returning an error when there is none.
    ///
    /// Mirrors Go's `VerifyQuickInfoExists` (`!quickInfoIsEmpty`): in the
    /// reachable subset, "exists" means [`Self::quick_info_at_current_position`]
    /// resolves a type string (`Some`), the analogue of Go's non-empty hover
    /// content.
    ///
    /// Side effects: as [`Self::quick_info_at_current_position`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyQuickInfoExists
    pub fn verify_quick_info_exists(&mut self) -> Result<(), FourslashError> {
        let prefix = self.current_position_prefix();
        match self.quick_info_at_current_position() {
            Some(_) => Ok(()),
            None => Err(FourslashError(format!(
                "{prefix}Expected non-nil hover content but got none"
            ))),
        }
    }

    /// Returns the language service's completion list at the **current** caret
    /// position, or `None` when the position is not a completion location (Go's
    /// `ProvideCompletion` returning a null response).
    ///
    /// The list is already sorted by label (the `tsgo_ls` reachable analogue of
    /// Go's `CompareCompletionEntries`), the same order Go's `getCompletions`
    /// re-sorts the server response into.
    ///
    /// Side effects: binds every program file and allocates a checker via the
    /// language service.
    // Go: internal/fourslash/fourslash.go:FourslashTest.getCompletions
    pub fn completions_at_current_position(&mut self) -> Option<CompletionList> {
        let file_name = self.active_filename.clone();
        let position = self.current_caret_position.clone();
        self.ls.provide_completions(&file_name, position)
    }

    /// Navigates to `marker_name`, requests completions there, and returns the
    /// assertion prefix plus the item labels (or `None` when the position is not
    /// a completion location).
    ///
    /// Side effects: as [`Self::completions_at_current_position`] (after moving
    /// the caret).
    // Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsWorker (label extraction)
    fn completions_labels_at(
        &mut self,
        marker_name: &str,
    ) -> Result<(String, Option<Vec<String>>), FourslashError> {
        self.go_to_marker(marker_name)?;
        let prefix = self.current_position_prefix();
        let labels = self
            .completions_at_current_position()
            .map(|list| list.items.iter().map(|item| item.label.clone()).collect());
        Ok((prefix, labels))
    }

    /// Verifies that the completions at `marker_name` *include* every label in
    /// `expected`, returning an error when any is missing.
    ///
    /// Mirrors the string form of Go's `VerifyCompletions` `Includes`: navigate
    /// to the marker, request completions, and assert each expected label is
    /// present in the actual items (`"Label '%s' not found in actual items."`).
    /// A `None` list (not a completion location) is an error unless `expected`
    /// is empty, mirroring Go's `verifyCompletionsResult` nil-list guard.
    ///
    /// Side effects: as [`Self::completions_labels_at`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsItems (Includes)
    pub fn verify_completions_include(
        &mut self,
        marker_name: &str,
        expected: &[&str],
    ) -> Result<(), FourslashError> {
        let (prefix, labels) = self.completions_labels_at(marker_name)?;
        let labels = match labels {
            Some(labels) => labels,
            None if expected.is_empty() => return Ok(()),
            None => {
                return Err(FourslashError(format!(
                    "{prefix}Expected completion list but got nil."
                )))
            }
        };
        for &name in expected {
            if !labels.iter().any(|label| label.as_str() == name) {
                return Err(FourslashError(format!(
                    "{prefix}Label '{name}' not found in actual items."
                )));
            }
        }
        Ok(())
    }

    /// Verifies that the completions at `marker_name` are *exactly* `expected`
    /// (same labels, in label order), returning an error otherwise.
    ///
    /// Mirrors Go's `VerifyCompletions` `Exact` via `verifyCompletionsAreExactly`
    /// (an ordered label comparison). The actual list is sorted by label, so
    /// `expected` must be given in label-sorted order. A `None` list compares as
    /// empty (an error when `expected` is non-empty, per Go's nil-list guard).
    ///
    /// Side effects: as [`Self::completions_labels_at`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsAreExactly
    pub fn verify_completions_exact(
        &mut self,
        marker_name: &str,
        expected: &[&str],
    ) -> Result<(), FourslashError> {
        let (prefix, labels) = self.completions_labels_at(marker_name)?;
        let labels = labels.unwrap_or_default();
        let actual: Vec<&str> = labels.iter().map(String::as_str).collect();
        if actual.as_slice() != expected {
            return Err(FourslashError(format!(
                "{prefix}Labels mismatch: expected {expected:?}, got {actual:?}"
            )));
        }
        Ok(())
    }

    /// Verifies that the completions at `marker_name` *exclude* every label in
    /// `excluded`, returning an error when any is present.
    ///
    /// Mirrors Go's `VerifyCompletions` `Excludes`
    /// (`"Label '%s' should not be in actual items but was found."`). A `None`
    /// list is an error unless `excluded` is empty, per Go's nil-list guard.
    ///
    /// Side effects: as [`Self::completions_labels_at`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.verifyCompletionsItems (Excludes)
    pub fn verify_completions_excludes(
        &mut self,
        marker_name: &str,
        excluded: &[&str],
    ) -> Result<(), FourslashError> {
        let (prefix, labels) = self.completions_labels_at(marker_name)?;
        let labels = match labels {
            Some(labels) => labels,
            None if excluded.is_empty() => return Ok(()),
            None => {
                return Err(FourslashError(format!(
                    "{prefix}Expected completion list but got nil."
                )))
            }
        };
        for &name in excluded {
            if labels.iter().any(|label| label.as_str() == name) {
                return Err(FourslashError(format!(
                    "{prefix}Label '{name}' should not be in actual items but was found."
                )));
            }
        }
        Ok(())
    }

    /// Verifies that go-to-definition from `from_marker` resolves to exactly the
    /// target markers `to_markers`, returning an error otherwise.
    ///
    /// Adapts the classic fourslash `verify.goToDefinition(from, to)` to drive
    /// the in-process LS (Go's `VerifyBaselineGoToDefinition` records a baseline;
    /// the baseline machinery is deferred — see the worklog DEFER list). Each
    /// target marker is placed at the start of a declaration's name, so this
    /// resolves the definitions at `from_marker` and asserts the multiset of
    /// definition `(uri, range.start)` equals the multiset of target marker
    /// `(uri, ls_position)`.
    ///
    /// Side effects: binds every program file and allocates a checker (after
    /// moving the caret).
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyBaselineGoToDefinition / verifyBaselineDefinitions
    pub fn verify_go_to_definition(
        &mut self,
        from_marker: &str,
        to_markers: &[&str],
    ) -> Result<(), FourslashError> {
        // Resolve the expected target `(uri, position)`s before taking the
        // `&mut self` borrow the provider needs.
        let mut expected: Vec<(String, Position)> = Vec::with_capacity(to_markers.len());
        for &name in to_markers {
            let marker = self
                .marker_by_name(name)
                .ok_or_else(|| FourslashError(format!("Marker '{name}' not found")))?;
            expected.push((
                file_name_to_document_uri(marker.file_name()).0,
                marker.ls_position.clone(),
            ));
        }

        self.go_to_marker(from_marker)?;
        let prefix = self.current_position_prefix();
        let file_name = self.active_filename.clone();
        let position = self.current_caret_position.clone();
        let locations = self.ls.provide_definition(&file_name, position);

        let mut actual: Vec<(String, Position)> = locations
            .iter()
            .map(|loc| (loc.uri.0.clone(), loc.range.start.clone()))
            .collect();
        sort_uri_positions(&mut actual);
        sort_uri_positions(&mut expected);
        if actual != expected {
            return Err(FourslashError(format!(
                "{prefix}Go-to-definition mismatch: expected {expected:?}, got {actual:?}"
            )));
        }
        Ok(())
    }

    /// Verifies that go-to-definition from `from_marker` resolves to the single
    /// target marker `to_marker` (the one-target convenience over
    /// [`Self::verify_go_to_definition`]).
    ///
    /// Side effects: as [`Self::verify_go_to_definition`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyBaselineGoToDefinition (single target)
    pub fn verify_definition_at(
        &mut self,
        from_marker: &str,
        to_marker: &str,
    ) -> Result<(), FourslashError> {
        self.verify_go_to_definition(from_marker, &[to_marker])
    }

    /// Verifies that find-all-references at `marker_name` returns exactly the
    /// locations `expected` (order-independent), returning an error otherwise.
    ///
    /// Mirrors Go's `VerifyBaselineFindAllReferences` semantics — resolve the
    /// symbol at the marker and collect every reference (including the
    /// declaration, Go's default `IncludeDeclaration`) — adapted to compare
    /// against the test's explicit `[|ranges|]` instead of recording a baseline
    /// (deferred — see the worklog DEFER list). Callers typically pass the
    /// `[|...|]` ranges mapped to [`RangeMarker::ls_location`].
    ///
    /// Side effects: binds every program file and allocates a checker (after
    /// moving the caret).
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyBaselineFindAllReferences
    pub fn verify_references_at(
        &mut self,
        marker_name: &str,
        expected: &[Location],
    ) -> Result<(), FourslashError> {
        self.go_to_marker(marker_name)?;
        let prefix = self.current_position_prefix();
        let file_name = self.active_filename.clone();
        let position = self.current_caret_position.clone();
        let mut actual = self.ls.provide_references(&file_name, position);
        let mut expected = expected.to_vec();
        sort_locations(&mut actual);
        sort_locations(&mut expected);
        if actual != expected {
            return Err(FourslashError(format!(
                "{prefix}Find-all-references mismatch: expected {expected:?}, got {actual:?}"
            )));
        }
        Ok(())
    }

    /// Returns the (syntactic + semantic) diagnostics for `file_name`, the
    /// reachable analogue of Go's combined `getDiagnostics`
    /// (`textDocument/diagnostic`).
    ///
    /// Side effects: reads parser diagnostics and drives the checker pool
    /// (idempotent) via the language service.
    // Go: internal/fourslash/fourslash.go:FourslashTest.getDiagnostics
    fn diagnostics_for_file(&mut self, file_name: &str) -> Vec<Diagnostic> {
        let mut diagnostics = self.ls.get_syntactic_diagnostics(file_name);
        diagnostics.extend(self.ls.get_semantic_diagnostics(file_name));
        diagnostics
    }

    /// Verifies that the current (active) file has exactly `expected_count`
    /// errors — diagnostics that are not suggestions/hints — returning an error
    /// otherwise.
    ///
    /// Mirrors Go's `VerifyNumberOfErrorsInCurrentFile`: fetch the active file's
    /// diagnostics, drop the suggestion (hint) severities, and compare the count.
    ///
    /// Side effects: as [`Self::diagnostics_for_file`].
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyNumberOfErrorsInCurrentFile
    pub fn verify_number_of_errors_in_current_file(
        &mut self,
        expected_count: usize,
    ) -> Result<(), FourslashError> {
        let file_name = self.active_filename.clone();
        let diagnostics = self.diagnostics_for_file(&file_name);
        let errors = diagnostics
            .iter()
            .filter(|diag| !is_suggestion_diagnostic(diag))
            .count();
        if errors != expected_count {
            return Err(FourslashError(format!(
                "Expected {expected_count} errors in current file, but got {errors}"
            )));
        }
        Ok(())
    }

    /// Verifies that no file in the test case has any error (non-suggestion)
    /// diagnostic, returning an error naming the first offending file otherwise.
    ///
    /// Mirrors Go's `VerifyNoErrors`, which iterates the open files; the
    /// reachable subset iterates every test file (each is "loaded" because the
    /// program holds them all).
    ///
    /// DEFER(phase-7-ls): per-file semantic-diagnostic partitioning. The
    /// compiler's semantic diagnostics are program-wide (no per-file split yet),
    /// so this is exact for single-user-file cases and over-broad for multi-file
    /// ones; blocked-by the same partition `tsgo_ls::get_semantic_diagnostics`
    /// notes.
    ///
    /// Side effects: as [`Self::diagnostics_for_file`], once per file.
    // Go: internal/fourslash/fourslash.go:FourslashTest.VerifyNoErrors
    pub fn verify_no_errors(&mut self) -> Result<(), FourslashError> {
        let file_names: Vec<String> = self
            .test_data
            .files
            .iter()
            .map(|file| file.file_name().to_string())
            .collect();
        for file_name in file_names {
            let diagnostics = self.diagnostics_for_file(&file_name);
            let errors: Vec<&Diagnostic> = diagnostics
                .iter()
                .filter(|diag| !is_suggestion_diagnostic(diag))
                .collect();
            if !errors.is_empty() {
                let messages: Vec<&str> = errors.iter().map(|diag| diag.message.as_str()).collect();
                return Err(FourslashError(format!(
                    "Expected no errors but found {} in {file_name}: {messages:?}",
                    errors.len()
                )));
            }
        }
        Ok(())
    }

    /// A human-readable prefix for assertion messages, naming the current marker
    /// or position.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/fourslash.go:FourslashTest.getCurrentPositionPrefix
    fn current_position_prefix(&self) -> String {
        match &self.last_known_marker_name {
            Some(name) => format!("At marker '{name}': "),
            None => format!(
                "At position {}(Ln {}, Col {}): ",
                self.active_filename,
                self.current_caret_position.line,
                self.current_caret_position.character
            ),
        }
    }
}

/// Sorts `(uri, position)` pairs by URI then `(line, character)`, so two
/// definition/target lists compare order-independently.
///
/// [`Position`] is only `PartialEq` (the `lsproto` objects derive no `Ord`), so
/// this sorts on its scalar fields rather than deriving an ordering.
///
/// Side effects: sorts `pairs` in place.
fn sort_uri_positions(pairs: &mut [(String, Position)]) {
    pairs.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.line.cmp(&b.1.line))
            .then(a.1.character.cmp(&b.1.character))
    });
}

/// Sorts [`Location`]s by URI then by start and end `(line, character)`, so two
/// reference lists compare order-independently.
///
/// Side effects: sorts `locations` in place.
fn sort_locations(locations: &mut [Location]) {
    locations.sort_by(|a, b| {
        a.uri
            .0
            .cmp(&b.uri.0)
            .then(a.range.start.line.cmp(&b.range.start.line))
            .then(a.range.start.character.cmp(&b.range.start.character))
            .then(a.range.end.line.cmp(&b.range.end.line))
            .then(a.range.end.character.cmp(&b.range.end.character))
    });
}

/// Reports whether `diagnostic` is a suggestion (hint) — the diagnostics Go's
/// error-counting verifies filter out.
///
/// Side effects: none (pure).
// Go: internal/fourslash/fourslash.go:isSuggestionDiagnostic
fn is_suggestion_diagnostic(diagnostic: &Diagnostic) -> bool {
    diagnostic.severity == Some(DiagnosticSeverity::HINT)
}

#[cfg(test)]
#[path = "driver_test.rs"]
mod tests;
