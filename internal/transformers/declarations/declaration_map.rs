//! Declaration source map (`.d.ts.map`) generation stub.
//!
//! A `.d.ts.map` file maps each declaration in the emitted `.d.ts` back to its
//! original source position. This module provides a thin wrapper around the
//! sourcemap [`Generator`](tsgo_sourcemap::Generator) specialized for
//! declaration emit. It records the source/declaration file metadata and
//! produces a [`RawSourceMap`](tsgo_sourcemap::RawSourceMap) with the correct
//! `file` (the `.d.ts` output) and `sources` (the original `.ts` input).
//!
//! # Scope (stub)
//!
//! This is an initial stub that wires up the generator and emits a structurally
//! valid (but empty-mappings) `.d.ts.map`. Mapping entries for individual
//! declarations will be added in a future round when the printer's source-map
//! hooks are wired into declaration emit.

use tsgo_sourcemap::{Generator, RawSourceMap, SourceIndex};
use tsgo_tspath::ComparePathsOptions;

/// A declaration-emit source map generator (Go's declaration-map support wired
/// through the printer's source-position hooks in `DeclarationTransformer`).
///
/// # Examples
/// ```
/// use tsgo_transformers::declarations::declaration_map::DeclarationMapGenerator;
/// let mut gen = DeclarationMapGenerator::new("/out/main.d.ts", "/src/main.ts");
/// let map = gen.to_raw_source_map();
/// assert_eq!(map.version, 3);
/// assert_eq!(map.file, "main.d.ts");
/// assert_eq!(map.sources, vec!["../src/main.ts"]);
/// ```
// Go: The declaration map is generated via the printer's sourceMapGenerator
// (internal/printer/printer.go) using the declaration transform's output path.
pub struct DeclarationMapGenerator {
    generator: Generator,
    source_index: SourceIndex,
}

impl DeclarationMapGenerator {
    /// Creates a declaration map generator for a `.d.ts` file at
    /// `declaration_file_path` whose original source is `source_file_path`.
    ///
    /// Both paths should be absolute or share a common root so the relative
    /// path computation produces a meaningful `sources` entry.
    ///
    /// Side effects: none (pure constructor).
    pub fn new(declaration_file_path: &str, source_file_path: &str) -> Self {
        let declaration_dir = dir_name(declaration_file_path);
        let declaration_file_name = base_name(declaration_file_path);

        let mut generator = Generator::new(
            declaration_file_name,
            "",
            &declaration_dir,
            ComparePathsOptions::default(),
        );

        let source_index = generator.add_source(source_file_path);

        DeclarationMapGenerator {
            generator,
            source_index,
        }
    }

    /// Returns the source index of the original `.ts` file in the source map
    /// (always `0` for a single-file declaration emit).
    pub fn source_index(&self) -> SourceIndex {
        self.source_index
    }

    /// Produces the serializable [`RawSourceMap`] for this declaration file.
    ///
    /// In this stub the `mappings` string is empty (no individual declaration
    /// positions are recorded yet). A future round will wire the printer's
    /// source-position hooks to record per-declaration mappings.
    pub fn to_raw_source_map(&mut self) -> RawSourceMap {
        self.generator.raw_source_map()
    }
}

/// Returns the directory portion of `path` (everything before the last `/`).
fn dir_name(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => ".".to_string(),
    }
}

/// Returns the file name portion of `path` (everything after the last `/`).
fn base_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

#[cfg(test)]
#[path = "declaration_map_test.rs"]
mod tests;
