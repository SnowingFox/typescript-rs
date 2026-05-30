//! The `CommandLineOption` declaration type, its kind/validation enums, and the
//! per-name associated maps (`Elements`/`EnumMap`/`DeprecatedKeys`).
//!
//! 1:1 port of Go `internal/tsoptions/commandlineoption.go`. Go's string
//! `CommandLineOptionKind` constants become a Rust enum; the `any`
//! `DefaultValueDescription` becomes [`DefaultValue`].
//!
//! DIVERGENCE(port): `Description`/`Category`/message-valued
//! `DefaultValueDescription` only feed `--help` and `--showConfig` output (both
//! deferred); they are left unset here and populated when those code paths land.
//! All fields that affect parsing/validation are populated exactly.

use std::sync::LazyLock;

use tsgo_collections::{OrderedMap, Set};
use tsgo_core::tristate::Tristate;
use tsgo_diagnostics::Message;

use crate::enummaps::{
    EnumValue, FALLBACK_ENUM_MAP, JSX_OPTION_MAP, LIB_MAP, MODULE_DETECTION_OPTION_MAP,
    MODULE_OPTION_MAP, MODULE_RESOLUTION_OPTION_MAP, NEW_LINE_OPTION_MAP, TARGET_OPTION_MAP,
    WATCH_DIRECTORY_ENUM_MAP, WATCH_FILE_ENUM_MAP,
};

/// The declared value kind of a [`CommandLineOption`].
///
/// Go compares the kind against string literals (`"boolean"`, `"enum"`, ...);
/// this port uses an enum discriminant instead.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::CommandLineOptionKind;
/// assert_eq!(CommandLineOptionKind::default(), CommandLineOptionKind::String);
/// ```
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommandLineOptionKind {
    /// A string value (`"string"`).
    #[default]
    String,
    /// A number value (`"number"`).
    Number,
    /// A boolean value (`"boolean"`).
    Boolean,
    /// An opaque object value (`"object"`), copied as-is.
    Object,
    /// A list value (`"list"`).
    List,
    /// A value that may be a single element or a list (`"listOrElement"`).
    ListOrElement,
    /// An enum value, looked up in an associated map (`"enum"`).
    Enum,
}

/// The extra validation a [`CommandLineOption`] requires beyond type-checking.
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExtraValidation {
    /// No extra validation.
    #[default]
    None,
    /// Validate as a file-glob spec (`specToDiagnostic`).
    Spec,
    /// Validate as a locale string.
    Locale,
}

/// The declared default-value description of an option.
///
/// Go stores this as `any`. In this port only the values consumed by parsing
/// are modeled precisely; message-valued descriptions (help text) are deferred
/// to [`DefaultValue::None`].
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DefaultValue {
    /// No description (or a deferred message-valued one).
    #[default]
    None,
    /// A boolean default.
    Bool(bool),
    /// A string default.
    Str(&'static str),
    /// A numeric default.
    Int(i64),
    /// A tri-state default (e.g. `core.TSUnknown`).
    Tristate(Tristate),
    /// A core-enum default, stored as its `i32` discriminant.
    Enum(i32),
}

/// A name-keyed map of option declarations, keyed by the lowercased option name.
///
/// Mirrors Go's `CommandLineOptionNameMap` (`map[string]*CommandLineOption`),
/// used both for `ElementOptions` and for the global compiler-options map.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/tsconfigparsing.go:CommandLineOptionNameMap
#[derive(Clone, Debug, Default)]
pub struct CommandLineOptionNameMap(pub OrderedMap<String, CommandLineOption>);

impl CommandLineOptionNameMap {
    /// Builds the map from a list of options, keyed by lowercased name.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/tsconfigparsing.go:commandLineOptionsToMap
    pub fn from_options(options: &[CommandLineOption]) -> Self {
        let mut m = OrderedMap::with_size_hint(options.len());
        for opt in options {
            m.set(opt.name.to_lowercase(), opt.clone());
        }
        CommandLineOptionNameMap(m)
    }

    /// Looks up an option by name (case-insensitively).
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/tsconfigparsing.go:CommandLineOptionNameMap.Get
    pub fn get(&self, name: &str) -> Option<&CommandLineOption> {
        self.0.get(&name.to_lowercase())
    }
}

/// A single compiler/watch/build option declaration.
///
/// 1:1 data port of Go's `CommandLineOption`. Pointer/`any` fields become
/// [`Option`]/typed enums; the associated `Elements`/`EnumMap`/`DeprecatedKeys`
/// lookups are exposed as methods.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/commandlineoption.go:CommandLineOption
#[derive(Clone, Debug, Default)]
pub struct CommandLineOption {
    /// Canonical option name.
    pub name: &'static str,
    /// Short alias (empty if none).
    pub short_name: &'static str,
    /// Declared value kind.
    pub kind: CommandLineOptionKind,

    /// Whether the value is a file path (made absolute during parsing).
    pub is_file_path: bool,
    /// Whether the option may only appear in `tsconfig.json`.
    pub is_tsconfig_only: bool,
    /// Whether the option may only appear on the command line.
    pub is_command_line_only: bool,

    /// Help description message (deferred; see module docs).
    pub description: Option<&'static Message>,
    /// Default-value description (parsing-relevant values only).
    pub default_value_description: DefaultValue,
    /// Whether the option is shown in the simplified `--help` view.
    pub show_in_simplified_help_view: bool,

    /// Help category message (deferred; see module docs).
    pub category: Option<&'static Message>,

    /// Extra validation performed by `validateJsonOptionValue`.
    pub extra_validation: ExtraValidation,

    /// Minimum permitted value for number options.
    pub min_value: i32,

    /// Whether `${configDir}` template substitution is allowed.
    pub allow_config_dir_template_substitution: bool,

    /// Whether changing this option affects the declaration output path.
    pub affects_declaration_path: bool,
    /// Whether changing this option affects program structure.
    pub affects_program_structure: bool,
    /// Whether changing this option affects semantic diagnostics.
    pub affects_semantic_diagnostics: bool,
    /// Whether changing this option affects build info.
    pub affects_build_info: bool,
    /// Whether changing this option affects bind diagnostics.
    pub affects_bind_diagnostics: bool,
    /// Whether changing this option affects source-file processing.
    pub affects_source_file: bool,
    /// Whether changing this option affects module resolution.
    pub affects_module_resolution: bool,
    /// Whether changing this option affects emit.
    pub affects_emit: bool,

    /// Whether this is the `allowJs` flag (special-cased in change detection).
    pub allow_js_flag: bool,
    /// Whether this is a `strict`-family flag.
    pub strict_flag: bool,

    /// The value forced when transpiling a single module.
    pub transpile_option_value: Tristate,

    /// Whether falsy list values are preserved (for list options).
    pub list_preserve_falsy_values: bool,

    /// Element/child option declarations (for object types).
    pub element_options: Option<Box<CommandLineOptionNameMap>>,
}

impl CommandLineOption {
    /// Returns the deprecated value keys for an enum option, or `None`.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/commandlineoption.go:CommandLineOption.DeprecatedKeys
    pub fn deprecated_keys(&self) -> Option<&'static Set<&'static str>> {
        if self.kind != CommandLineOptionKind::Enum {
            return None;
        }
        COMMAND_LINE_OPTION_DEPRECATED.get(&self.name)
    }

    /// Returns the value-name map for an enum option, or `None`.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/commandlineoption.go:CommandLineOption.EnumMap
    pub fn enum_map(&self) -> Option<&'static OrderedMap<&'static str, EnumValue>> {
        if self.kind != CommandLineOptionKind::Enum {
            return None;
        }
        command_line_option_enum_map(self.name)
    }

    /// Returns the element declaration for a list/listOrElement option.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/commandlineoption.go:CommandLineOption.Elements
    pub fn elements(&self) -> Option<&'static CommandLineOption> {
        if self.kind != CommandLineOptionKind::List
            && self.kind != CommandLineOptionKind::ListOrElement
        {
            return None;
        }
        COMMAND_LINE_OPTION_ELEMENTS.get(&self.name)
    }

    /// Reports whether `null`/`undefined` are disallowed for this option (only
    /// `extends`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_tsoptions::{CommandLineOption, CommandLineOptionKind};
    /// let mut o = CommandLineOption { name: "extends", ..Default::default() };
    /// assert!(o.disallow_null_or_undefined());
    /// o.name = "lib";
    /// assert!(!o.disallow_null_or_undefined());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/commandlineoption.go:CommandLineOption.DisallowNullOrUndefined
    pub fn disallow_null_or_undefined(&self) -> bool {
        self.name == "extends"
    }
}

/// The per-name `Elements()` declarations for list/listOrElement options.
// Go: internal/tsoptions/commandlineoption.go:commandLineOptionElements
static COMMAND_LINE_OPTION_ELEMENTS: LazyLock<OrderedMap<&'static str, CommandLineOption>> =
    LazyLock::new(|| {
        let entries: &[(&'static str, CommandLineOption)] = &[
            (
                "lib",
                CommandLineOption {
                    name: "lib",
                    kind: CommandLineOptionKind::Enum, // libMap
                    default_value_description: DefaultValue::Tristate(Tristate::Unknown),
                    ..Default::default()
                },
            ),
            (
                "rootDirs",
                CommandLineOption {
                    name: "rootDirs",
                    kind: CommandLineOptionKind::String,
                    is_file_path: true,
                    ..Default::default()
                },
            ),
            (
                "typeRoots",
                CommandLineOption {
                    name: "typeRoots",
                    kind: CommandLineOptionKind::String,
                    is_file_path: true,
                    ..Default::default()
                },
            ),
            (
                "types",
                CommandLineOption {
                    name: "types",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
            (
                "moduleSuffixes",
                CommandLineOption {
                    name: "moduleSuffixes",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
            (
                "customConditions",
                CommandLineOption {
                    name: "condition",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
            (
                "plugins",
                CommandLineOption {
                    name: "plugin",
                    kind: CommandLineOptionKind::Object,
                    ..Default::default()
                },
            ),
            // For tsconfig root options
            (
                "references",
                CommandLineOption {
                    name: "references",
                    kind: CommandLineOptionKind::Object,
                    ..Default::default()
                },
            ),
            (
                "files",
                CommandLineOption {
                    name: "files",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
            (
                "include",
                CommandLineOption {
                    name: "include",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
            (
                "exclude",
                CommandLineOption {
                    name: "exclude",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
            (
                "extends",
                CommandLineOption {
                    name: "extends",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
            // For watch options
            (
                "excludeDirectories",
                CommandLineOption {
                    name: "excludeDirectory",
                    kind: CommandLineOptionKind::String,
                    is_file_path: true,
                    extra_validation: ExtraValidation::Spec,
                    ..Default::default()
                },
            ),
            (
                "excludeFiles",
                CommandLineOption {
                    name: "excludeFile",
                    kind: CommandLineOptionKind::String,
                    is_file_path: true,
                    extra_validation: ExtraValidation::Spec,
                    ..Default::default()
                },
            ),
            // Test infra options
            (
                "libFiles",
                CommandLineOption {
                    name: "libFiles",
                    kind: CommandLineOptionKind::String,
                    ..Default::default()
                },
            ),
        ];
        let mut m = OrderedMap::with_size_hint(entries.len());
        for (k, v) in entries {
            m.set(*k, v.clone());
        }
        m
    });

/// Returns the enum value map associated with the named enum option.
// Go: internal/tsoptions/commandlineoption.go:commandLineOptionEnumMap
fn command_line_option_enum_map(
    name: &str,
) -> Option<&'static OrderedMap<&'static str, EnumValue>> {
    match name {
        "lib" => Some(&LIB_MAP),
        "moduleResolution" => Some(&MODULE_RESOLUTION_OPTION_MAP),
        "module" => Some(&MODULE_OPTION_MAP),
        "target" => Some(&TARGET_OPTION_MAP),
        "moduleDetection" => Some(&MODULE_DETECTION_OPTION_MAP),
        "jsx" => Some(&JSX_OPTION_MAP),
        "newLine" => Some(&NEW_LINE_OPTION_MAP),
        "watchFile" => Some(&WATCH_FILE_ENUM_MAP),
        "watchDirectory" => Some(&WATCH_DIRECTORY_ENUM_MAP),
        "fallbackPolling" => Some(&FALLBACK_ENUM_MAP),
        _ => None,
    }
}

/// The per-name deprecated value keys for enum options.
// Go: internal/tsoptions/commandlineoption.go:commandLineOptionDeprecated
static COMMAND_LINE_OPTION_DEPRECATED: LazyLock<OrderedMap<&'static str, Set<&'static str>>> =
    LazyLock::new(|| {
        let mut m = OrderedMap::with_size_hint(3);
        m.set("module", Set::from_items(["none", "amd", "system", "umd"]));
        m.set(
            "moduleResolution",
            Set::from_items(["node", "classic", "node10"]),
        );
        m.set("target", Set::from_items(["es5"]));
        m
    });

#[cfg(test)]
#[path = "commandlineoption_test.rs"]
mod tests;
