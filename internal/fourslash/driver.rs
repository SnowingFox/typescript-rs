//! Port of Go `internal/fourslash/fourslash.go`: the [`FourslashTest`] driver
//! skeleton (foundation round).
//!
//! Given parsed [`TestData`], [`new_fourslash`] builds an in-memory
//! [`LanguageService`] over the marker-stripped files and exposes the caret
//! navigation primitive ([`FourslashTest::go_to_marker`]) plus a first verify
//! command ([`FourslashTest::verify_quick_info_at`]).
//!
//! # Divergence from Go (foundation round)
//!
//! Go's `FourslashTest` drives an in-memory **LSP server** over channels
//! (`lsptestutil.NewLSPClient`, backed by `internal/lsp` + `internal/project`)
//! and "opens" files with `textDocument/didOpen` notifications. Those crates
//! are P8 and not yet ported, so this foundation builds and drives the
//! in-process [`tsgo_ls::LanguageService`] directly — the same way the `tsgo_ls`
//! feature tests construct a service over an in-memory program. Consequences:
//! - `go_to_marker` only moves the caret state; "opening" a file is a no-op
//!   because the program already holds every file (Go sends `didOpen`).
//! - `verify_quick_info_at` calls [`LanguageService::get_quick_info_at_position`]
//!   and compares the reachable type string (Go compares the full markdown
//!   hover body, which `tsgo_ls` has not yet ported — see its `hover` module).
//! - Symlinks and dynamic (`untitled:`) files are not added to the VFS this
//!   round (no smoke case needs them).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use tsgo_compiler::{new_compiler_host, new_program, ProgramOptions};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_ls::{LanguageService, LanguageServiceHost, QuickInfo};
use tsgo_lsproto::Position;
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

#[cfg(test)]
#[path = "driver_test.rs"]
mod tests;
