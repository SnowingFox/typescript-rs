//! Name maps from option name / short name to declaration, used by the command
//! line parser.
//!
//! 1:1 port of Go `internal/tsoptions/namemap.go`. Go stores `*CommandLineOption`
//! pointers; this port owns cloned declarations (lookups return borrows).

use std::collections::HashMap;
use std::sync::LazyLock;

use tsgo_collections::OrderedMap;

use crate::commandlineoption::CommandLineOption;
use crate::declsbuild::BUILD_OPTS;
use crate::declscompiler::OPTIONS_DECLARATIONS;
use crate::declswatch::OPTIONS_FOR_WATCH;

/// Maps lowercased option names (and short-name aliases) to declarations.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/namemap.go:NameMap
#[derive(Clone, Debug, Default)]
pub struct NameMap {
    options_names: OrderedMap<String, CommandLineOption>,
    short_option_names: HashMap<String, String>,
}

/// Builds a [`NameMap`] from a list of declarations.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{get_name_map_from_list, CommandLineOption, CommandLineOptionKind};
/// let decls = vec![CommandLineOption {
///     name: "target",
///     short_name: "t",
///     kind: CommandLineOptionKind::Enum,
///     ..Default::default()
/// }];
/// let nm = get_name_map_from_list(&decls);
/// assert_eq!(nm.get("TARGET").map(|o| o.name), Some("target"));
/// assert_eq!(nm.get_from_short("t").map(|o| o.name), Some("target"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/namemap.go:GetNameMapFromList
pub fn get_name_map_from_list(opt_decls: &[CommandLineOption]) -> NameMap {
    let mut options_names = OrderedMap::with_size_hint(opt_decls.len());
    let mut short_option_names = HashMap::new();
    for option in opt_decls {
        options_names.set(option.name.to_lowercase(), option.clone());
        if !option.short_name.is_empty() {
            short_option_names.insert(option.short_name.to_string(), option.name.to_string());
        }
    }
    NameMap {
        options_names,
        short_option_names,
    }
}

impl NameMap {
    /// Looks up an option by name (case-insensitively).
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/namemap.go:NameMap.Get
    pub fn get(&self, name: &str) -> Option<&CommandLineOption> {
        self.options_names.get(&name.to_lowercase())
    }

    /// Looks up an option by its short-name alias, if valid.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/namemap.go:NameMap.GetFromShort
    pub fn get_from_short(&self, short_name: &str) -> Option<&CommandLineOption> {
        let name = self.short_option_names.get(short_name)?;
        self.get(name)
    }

    /// Looks up an option by name, optionally translating short names first.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/namemap.go:NameMap.GetOptionDeclarationFromName
    pub fn get_option_declaration_from_name(
        &self,
        option_name: &str,
        allow_short: bool,
    ) -> Option<&CommandLineOption> {
        let mut option_name = option_name.to_lowercase();
        if allow_short {
            if let Some(full) = self.short_option_names.get(&option_name) {
                option_name = full.clone();
            }
        }
        self.get(&option_name)
    }
}

/// Name map over all compiler option declarations.
// Go: internal/tsoptions/namemap.go:CompilerNameMap
pub static COMPILER_NAME_MAP: LazyLock<NameMap> =
    LazyLock::new(|| get_name_map_from_list(&OPTIONS_DECLARATIONS));

/// Name map over all `tsc --build` option declarations.
// Go: internal/tsoptions/namemap.go:BuildNameMap
pub static BUILD_NAME_MAP: LazyLock<NameMap> =
    LazyLock::new(|| get_name_map_from_list(&BUILD_OPTS));

/// Name map over all watch option declarations.
// Go: internal/tsoptions/namemap.go:WatchNameMap
pub static WATCH_NAME_MAP: LazyLock<NameMap> =
    LazyLock::new(|| get_name_map_from_list(&OPTIONS_FOR_WATCH));

#[cfg(test)]
#[path = "namemap_test.rs"]
mod tests;
