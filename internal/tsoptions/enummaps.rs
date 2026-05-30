//! Enum/name maps for option values: `lib` names, target/module/JSX kinds,
//! watch kinds, plus the default-lib-file lookup.
//!
//! 1:1 port of Go `internal/tsoptions/enummaps.go`. Go stores these as
//! `*collections.OrderedMap[string, any]`; here the `any` values are modeled by
//! [`EnumValue`] (an `i32` enum discriminant or a `'static` string for `lib`
//! filenames), keeping insertion order which several callers depend on
//! ("did you mean" lists, `--init` ordering).

use std::sync::LazyLock;

use tsgo_collections::{MapEntry, OrderedMap, Set};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::compileroptions::{
    JsxEmit, ModuleDetectionKind, ModuleKind, ModuleResolutionKind, NewLineKind, ScriptTarget,
};
use tsgo_core::watchoptions::{PollingKind, WatchDirectoryKind, WatchFileKind};

/// A value held in an option enum map.
///
/// Go uses `any`; the concrete values are either one of the `i32`-repr `core`
/// enums (stored as their discriminant) or, for the `lib` map, a `'static`
/// declaration-file name.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::EnumValue;
/// assert_eq!(EnumValue::Int(5).as_int(), Some(5));
/// assert_eq!(EnumValue::Str("lib.es5.d.ts").as_str(), Some("lib.es5.d.ts"));
/// ```
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumValue {
    /// An `i32`-repr enum discriminant (target/module/jsx/...).
    Int(i32),
    /// A `'static` string value (used by the `lib` map for file names).
    Str(&'static str),
}

impl EnumValue {
    /// Returns the discriminant if this is [`EnumValue::Int`].
    ///
    /// Side effects: none (pure).
    pub fn as_int(self) -> Option<i32> {
        match self {
            EnumValue::Int(v) => Some(v),
            EnumValue::Str(_) => None,
        }
    }

    /// Returns the string if this is [`EnumValue::Str`].
    ///
    /// Side effects: none (pure).
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            EnumValue::Str(s) => Some(s),
            EnumValue::Int(_) => None,
        }
    }
}

/// Builds an [`OrderedMap`] of `&'static str` -> [`EnumValue`] from a list of
/// `(key, value)` pairs, preserving order.
fn enum_map(items: &[(&'static str, EnumValue)]) -> OrderedMap<&'static str, EnumValue> {
    OrderedMap::from_list(
        items
            .iter()
            .map(|(k, v)| MapEntry { key: *k, value: *v })
            .collect(),
    )
}

/// The `lib` short-name -> declaration-file-name map, insertion-ordered.
// Go: internal/tsoptions/enummaps.go:LibMap
pub static LIB_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> = LazyLock::new(|| {
    enum_map(&[
        // JavaScript only
        ("es5", EnumValue::Str("lib.es5.d.ts")),
        ("es6", EnumValue::Str("lib.es2015.d.ts")),
        ("es2015", EnumValue::Str("lib.es2015.d.ts")),
        ("es7", EnumValue::Str("lib.es2016.d.ts")),
        ("es2016", EnumValue::Str("lib.es2016.d.ts")),
        ("es2017", EnumValue::Str("lib.es2017.d.ts")),
        ("es2018", EnumValue::Str("lib.es2018.d.ts")),
        ("es2019", EnumValue::Str("lib.es2019.d.ts")),
        ("es2020", EnumValue::Str("lib.es2020.d.ts")),
        ("es2021", EnumValue::Str("lib.es2021.d.ts")),
        ("es2022", EnumValue::Str("lib.es2022.d.ts")),
        ("es2023", EnumValue::Str("lib.es2023.d.ts")),
        ("es2024", EnumValue::Str("lib.es2024.d.ts")),
        ("es2025", EnumValue::Str("lib.es2025.d.ts")),
        ("esnext", EnumValue::Str("lib.esnext.d.ts")),
        // Host only
        ("dom", EnumValue::Str("lib.dom.d.ts")),
        ("dom.iterable", EnumValue::Str("lib.dom.iterable.d.ts")),
        (
            "dom.asynciterable",
            EnumValue::Str("lib.dom.asynciterable.d.ts"),
        ),
        ("webworker", EnumValue::Str("lib.webworker.d.ts")),
        (
            "webworker.importscripts",
            EnumValue::Str("lib.webworker.importscripts.d.ts"),
        ),
        (
            "webworker.iterable",
            EnumValue::Str("lib.webworker.iterable.d.ts"),
        ),
        (
            "webworker.asynciterable",
            EnumValue::Str("lib.webworker.asynciterable.d.ts"),
        ),
        ("scripthost", EnumValue::Str("lib.scripthost.d.ts")),
        // ES2015 and later By-feature options
        ("es2015.core", EnumValue::Str("lib.es2015.core.d.ts")),
        (
            "es2015.collection",
            EnumValue::Str("lib.es2015.collection.d.ts"),
        ),
        (
            "es2015.generator",
            EnumValue::Str("lib.es2015.generator.d.ts"),
        ),
        (
            "es2015.iterable",
            EnumValue::Str("lib.es2015.iterable.d.ts"),
        ),
        ("es2015.promise", EnumValue::Str("lib.es2015.promise.d.ts")),
        ("es2015.proxy", EnumValue::Str("lib.es2015.proxy.d.ts")),
        ("es2015.reflect", EnumValue::Str("lib.es2015.reflect.d.ts")),
        ("es2015.symbol", EnumValue::Str("lib.es2015.symbol.d.ts")),
        (
            "es2015.symbol.wellknown",
            EnumValue::Str("lib.es2015.symbol.wellknown.d.ts"),
        ),
        (
            "es2016.array.include",
            EnumValue::Str("lib.es2016.array.include.d.ts"),
        ),
        ("es2016.intl", EnumValue::Str("lib.es2016.intl.d.ts")),
        (
            "es2017.arraybuffer",
            EnumValue::Str("lib.es2017.arraybuffer.d.ts"),
        ),
        ("es2017.date", EnumValue::Str("lib.es2017.date.d.ts")),
        ("es2017.object", EnumValue::Str("lib.es2017.object.d.ts")),
        (
            "es2017.sharedmemory",
            EnumValue::Str("lib.es2017.sharedmemory.d.ts"),
        ),
        ("es2017.string", EnumValue::Str("lib.es2017.string.d.ts")),
        ("es2017.intl", EnumValue::Str("lib.es2017.intl.d.ts")),
        (
            "es2017.typedarrays",
            EnumValue::Str("lib.es2017.typedarrays.d.ts"),
        ),
        (
            "es2018.asyncgenerator",
            EnumValue::Str("lib.es2018.asyncgenerator.d.ts"),
        ),
        (
            "es2018.asynciterable",
            EnumValue::Str("lib.es2018.asynciterable.d.ts"),
        ),
        ("es2018.intl", EnumValue::Str("lib.es2018.intl.d.ts")),
        ("es2018.promise", EnumValue::Str("lib.es2018.promise.d.ts")),
        ("es2018.regexp", EnumValue::Str("lib.es2018.regexp.d.ts")),
        ("es2019.array", EnumValue::Str("lib.es2019.array.d.ts")),
        ("es2019.object", EnumValue::Str("lib.es2019.object.d.ts")),
        ("es2019.string", EnumValue::Str("lib.es2019.string.d.ts")),
        ("es2019.symbol", EnumValue::Str("lib.es2019.symbol.d.ts")),
        ("es2019.intl", EnumValue::Str("lib.es2019.intl.d.ts")),
        ("es2020.bigint", EnumValue::Str("lib.es2020.bigint.d.ts")),
        ("es2020.date", EnumValue::Str("lib.es2020.date.d.ts")),
        ("es2020.promise", EnumValue::Str("lib.es2020.promise.d.ts")),
        (
            "es2020.sharedmemory",
            EnumValue::Str("lib.es2020.sharedmemory.d.ts"),
        ),
        ("es2020.string", EnumValue::Str("lib.es2020.string.d.ts")),
        (
            "es2020.symbol.wellknown",
            EnumValue::Str("lib.es2020.symbol.wellknown.d.ts"),
        ),
        ("es2020.intl", EnumValue::Str("lib.es2020.intl.d.ts")),
        ("es2020.number", EnumValue::Str("lib.es2020.number.d.ts")),
        ("es2021.promise", EnumValue::Str("lib.es2021.promise.d.ts")),
        ("es2021.string", EnumValue::Str("lib.es2021.string.d.ts")),
        ("es2021.weakref", EnumValue::Str("lib.es2021.weakref.d.ts")),
        ("es2021.intl", EnumValue::Str("lib.es2021.intl.d.ts")),
        ("es2022.array", EnumValue::Str("lib.es2022.array.d.ts")),
        ("es2022.error", EnumValue::Str("lib.es2022.error.d.ts")),
        ("es2022.intl", EnumValue::Str("lib.es2022.intl.d.ts")),
        ("es2022.object", EnumValue::Str("lib.es2022.object.d.ts")),
        ("es2022.string", EnumValue::Str("lib.es2022.string.d.ts")),
        ("es2022.regexp", EnumValue::Str("lib.es2022.regexp.d.ts")),
        ("es2023.array", EnumValue::Str("lib.es2023.array.d.ts")),
        (
            "es2023.collection",
            EnumValue::Str("lib.es2023.collection.d.ts"),
        ),
        ("es2023.intl", EnumValue::Str("lib.es2023.intl.d.ts")),
        (
            "es2024.arraybuffer",
            EnumValue::Str("lib.es2024.arraybuffer.d.ts"),
        ),
        (
            "es2024.collection",
            EnumValue::Str("lib.es2024.collection.d.ts"),
        ),
        ("es2024.object", EnumValue::Str("lib.es2024.object.d.ts")),
        ("es2024.promise", EnumValue::Str("lib.es2024.promise.d.ts")),
        ("es2024.regexp", EnumValue::Str("lib.es2024.regexp.d.ts")),
        (
            "es2024.sharedmemory",
            EnumValue::Str("lib.es2024.sharedmemory.d.ts"),
        ),
        ("es2024.string", EnumValue::Str("lib.es2024.string.d.ts")),
        (
            "es2025.collection",
            EnumValue::Str("lib.es2025.collection.d.ts"),
        ),
        ("es2025.float16", EnumValue::Str("lib.es2025.float16.d.ts")),
        ("es2025.intl", EnumValue::Str("lib.es2025.intl.d.ts")),
        (
            "es2025.iterator",
            EnumValue::Str("lib.es2025.iterator.d.ts"),
        ),
        ("es2025.promise", EnumValue::Str("lib.es2025.promise.d.ts")),
        ("es2025.regexp", EnumValue::Str("lib.es2025.regexp.d.ts")),
        // Fallback for backward compatibility
        (
            "esnext.asynciterable",
            EnumValue::Str("lib.es2018.asynciterable.d.ts"),
        ),
        ("esnext.symbol", EnumValue::Str("lib.es2019.symbol.d.ts")),
        ("esnext.bigint", EnumValue::Str("lib.es2020.bigint.d.ts")),
        ("esnext.weakref", EnumValue::Str("lib.es2021.weakref.d.ts")),
        ("esnext.object", EnumValue::Str("lib.es2024.object.d.ts")),
        ("esnext.regexp", EnumValue::Str("lib.es2024.regexp.d.ts")),
        ("esnext.string", EnumValue::Str("lib.es2024.string.d.ts")),
        ("esnext.float16", EnumValue::Str("lib.es2025.float16.d.ts")),
        (
            "esnext.iterator",
            EnumValue::Str("lib.es2025.iterator.d.ts"),
        ),
        ("esnext.promise", EnumValue::Str("lib.es2025.promise.d.ts")),
        // ESNext By-feature options
        ("esnext.array", EnumValue::Str("lib.esnext.array.d.ts")),
        (
            "esnext.collection",
            EnumValue::Str("lib.esnext.collection.d.ts"),
        ),
        ("esnext.date", EnumValue::Str("lib.esnext.date.d.ts")),
        (
            "esnext.decorators",
            EnumValue::Str("lib.esnext.decorators.d.ts"),
        ),
        (
            "esnext.disposable",
            EnumValue::Str("lib.esnext.disposable.d.ts"),
        ),
        ("esnext.error", EnumValue::Str("lib.esnext.error.d.ts")),
        ("esnext.intl", EnumValue::Str("lib.esnext.intl.d.ts")),
        (
            "esnext.sharedmemory",
            EnumValue::Str("lib.esnext.sharedmemory.d.ts"),
        ),
        (
            "esnext.temporal",
            EnumValue::Str("lib.esnext.temporal.d.ts"),
        ),
        (
            "esnext.typedarrays",
            EnumValue::Str("lib.esnext.typedarrays.d.ts"),
        ),
        // Decorators
        ("decorators", EnumValue::Str("lib.decorators.d.ts")),
        (
            "decorators.legacy",
            EnumValue::Str("lib.decorators.legacy.d.ts"),
        ),
    ])
});

/// All `lib` short names in declaration order.
// Go: internal/tsoptions/enummaps.go:Libs
pub static LIBS: LazyLock<Vec<&'static str>> = LazyLock::new(|| LIB_MAP.keys().copied().collect());

/// The set of all canonical `lib` declaration-file names.
// Go: internal/tsoptions/enummaps.go:LibFilesSet
pub static LIB_FILES_SET: LazyLock<Set<&'static str>> =
    LazyLock::new(|| Set::from_items(LIB_MAP.values().filter_map(|v| v.as_str())));

/// Returns the canonical declaration-file name for a `lib` reference, accepting
/// either a known lib short name (e.g. `"es6"`) or a known file name directly.
///
/// The name is lowercased first (matching Go's `ToFileNameLowerCase`). Returns
/// `None` for an unknown name.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::get_lib_file_name;
/// assert_eq!(get_lib_file_name("es6").as_deref(), Some("lib.es2015.d.ts"));
/// assert_eq!(get_lib_file_name("lib.es5.d.ts").as_deref(), Some("lib.es5.d.ts"));
/// assert_eq!(get_lib_file_name("nope"), None);
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/enummaps.go:GetLibFileName
pub fn get_lib_file_name(lib_name: &str) -> Option<String> {
    let lib_name = tsgo_tspath::to_file_name_lower_case(lib_name);
    if LIB_FILES_SET.has(&lib_name.as_str()) {
        return Some(lib_name);
    }
    LIB_MAP
        .get(&lib_name.as_str())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// The `moduleResolution` name -> [`ModuleResolutionKind`] map.
// Go: internal/tsoptions/enummaps.go:moduleResolutionOptionMap
pub static MODULE_RESOLUTION_OPTION_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> =
    LazyLock::new(|| {
        enum_map(&[
            (
                "node16",
                EnumValue::Int(ModuleResolutionKind::Node16 as i32),
            ),
            (
                "nodenext",
                EnumValue::Int(ModuleResolutionKind::NodeNext as i32),
            ),
            (
                "bundler",
                EnumValue::Int(ModuleResolutionKind::Bundler as i32),
            ),
            (
                "classic",
                EnumValue::Int(ModuleResolutionKind::Classic as i32),
            ),
            ("node", EnumValue::Int(ModuleResolutionKind::Node10 as i32)),
            (
                "node10",
                EnumValue::Int(ModuleResolutionKind::Node10 as i32),
            ),
        ])
    });

/// The `target` name -> [`ScriptTarget`] map.
// Go: internal/tsoptions/enummaps.go:targetOptionMap
pub static TARGET_OPTION_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> = LazyLock::new(|| {
    enum_map(&[
        ("es5", EnumValue::Int(ScriptTarget::Es5 as i32)),
        ("es6", EnumValue::Int(ScriptTarget::Es2015 as i32)),
        ("es2015", EnumValue::Int(ScriptTarget::Es2015 as i32)),
        ("es2016", EnumValue::Int(ScriptTarget::Es2016 as i32)),
        ("es2017", EnumValue::Int(ScriptTarget::Es2017 as i32)),
        ("es2018", EnumValue::Int(ScriptTarget::Es2018 as i32)),
        ("es2019", EnumValue::Int(ScriptTarget::Es2019 as i32)),
        ("es2020", EnumValue::Int(ScriptTarget::Es2020 as i32)),
        ("es2021", EnumValue::Int(ScriptTarget::Es2021 as i32)),
        ("es2022", EnumValue::Int(ScriptTarget::Es2022 as i32)),
        ("es2023", EnumValue::Int(ScriptTarget::Es2023 as i32)),
        ("es2024", EnumValue::Int(ScriptTarget::Es2024 as i32)),
        ("es2025", EnumValue::Int(ScriptTarget::Es2025 as i32)),
        ("esnext", EnumValue::Int(ScriptTarget::EsNext as i32)),
    ])
});

/// The `module` name -> [`ModuleKind`] map.
// Go: internal/tsoptions/enummaps.go:moduleOptionMap
pub static MODULE_OPTION_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> = LazyLock::new(|| {
    enum_map(&[
        ("commonjs", EnumValue::Int(ModuleKind::CommonJs as i32)),
        ("amd", EnumValue::Int(ModuleKind::Amd as i32)),
        ("system", EnumValue::Int(ModuleKind::System as i32)),
        ("umd", EnumValue::Int(ModuleKind::Umd as i32)),
        ("es6", EnumValue::Int(ModuleKind::Es2015 as i32)),
        ("es2015", EnumValue::Int(ModuleKind::Es2015 as i32)),
        ("es2020", EnumValue::Int(ModuleKind::Es2020 as i32)),
        ("es2022", EnumValue::Int(ModuleKind::Es2022 as i32)),
        ("esnext", EnumValue::Int(ModuleKind::EsNext as i32)),
        ("node16", EnumValue::Int(ModuleKind::Node16 as i32)),
        ("node18", EnumValue::Int(ModuleKind::Node18 as i32)),
        ("node20", EnumValue::Int(ModuleKind::Node20 as i32)),
        ("nodenext", EnumValue::Int(ModuleKind::NodeNext as i32)),
        ("preserve", EnumValue::Int(ModuleKind::Preserve as i32)),
    ])
});

/// The `moduleDetection` name -> [`ModuleDetectionKind`] map.
// Go: internal/tsoptions/enummaps.go:moduleDetectionOptionMap
pub static MODULE_DETECTION_OPTION_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> =
    LazyLock::new(|| {
        enum_map(&[
            ("auto", EnumValue::Int(ModuleDetectionKind::Auto as i32)),
            ("legacy", EnumValue::Int(ModuleDetectionKind::Legacy as i32)),
            ("force", EnumValue::Int(ModuleDetectionKind::Force as i32)),
        ])
    });

/// The `jsx` name -> [`JsxEmit`] map.
// Go: internal/tsoptions/enummaps.go:jsxOptionMap
pub static JSX_OPTION_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> = LazyLock::new(|| {
    enum_map(&[
        ("preserve", EnumValue::Int(JsxEmit::Preserve as i32)),
        ("react-native", EnumValue::Int(JsxEmit::ReactNative as i32)),
        ("react-jsx", EnumValue::Int(JsxEmit::ReactJsx as i32)),
        ("react-jsxdev", EnumValue::Int(JsxEmit::ReactJsxDev as i32)),
        ("react", EnumValue::Int(JsxEmit::React as i32)),
    ])
});

/// The `newLine` name -> [`NewLineKind`] map.
// Go: internal/tsoptions/enummaps.go:newLineOptionMap
pub static NEW_LINE_OPTION_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> =
    LazyLock::new(|| {
        enum_map(&[
            ("crlf", EnumValue::Int(NewLineKind::Crlf as i32)),
            ("lf", EnumValue::Int(NewLineKind::Lf as i32)),
        ])
    });

/// The `watchFile` name -> [`WatchFileKind`] map.
// Go: internal/tsoptions/enummaps.go:watchFileEnumMap
pub static WATCH_FILE_ENUM_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> =
    LazyLock::new(|| {
        enum_map(&[
            (
                "fixedpollinginterval",
                EnumValue::Int(WatchFileKind::FixedPollingInterval as i32),
            ),
            (
                "prioritypollinginterval",
                EnumValue::Int(WatchFileKind::PriorityPollingInterval as i32),
            ),
            (
                "dynamicprioritypolling",
                EnumValue::Int(WatchFileKind::DynamicPriorityPolling as i32),
            ),
            (
                "fixedchunksizepolling",
                EnumValue::Int(WatchFileKind::FixedChunkSizePolling as i32),
            ),
            (
                "usefsevents",
                EnumValue::Int(WatchFileKind::UseFsEvents as i32),
            ),
            (
                "usefseventsonparentdirectory",
                EnumValue::Int(WatchFileKind::UseFsEventsOnParentDirectory as i32),
            ),
        ])
    });

/// The `watchDirectory` name -> [`WatchDirectoryKind`] map.
// Go: internal/tsoptions/enummaps.go:watchDirectoryEnumMap
pub static WATCH_DIRECTORY_ENUM_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> =
    LazyLock::new(|| {
        enum_map(&[
            (
                "usefsevents",
                EnumValue::Int(WatchDirectoryKind::UseFsEvents as i32),
            ),
            (
                "fixedpollinginterval",
                EnumValue::Int(WatchDirectoryKind::FixedPollingInterval as i32),
            ),
            (
                "dynamicprioritypolling",
                EnumValue::Int(WatchDirectoryKind::DynamicPriorityPolling as i32),
            ),
            (
                "fixedchunksizepolling",
                EnumValue::Int(WatchDirectoryKind::FixedChunkSizePolling as i32),
            ),
        ])
    });

/// The `fallbackPolling` name -> [`PollingKind`] map.
// Go: internal/tsoptions/enummaps.go:fallbackEnumMap
pub static FALLBACK_ENUM_MAP: LazyLock<OrderedMap<&'static str, EnumValue>> = LazyLock::new(|| {
    enum_map(&[
        (
            "fixedinterval",
            EnumValue::Int(PollingKind::FixedInterval as i32),
        ),
        (
            "priorityinterval",
            EnumValue::Int(PollingKind::PriorityInterval as i32),
        ),
        (
            "dynamicpriority",
            EnumValue::Int(PollingKind::DynamicPriority as i32),
        ),
        (
            "fixedchunksize",
            EnumValue::Int(PollingKind::FixedChunkSize as i32),
        ),
    ])
});

/// Returns the [`ScriptTarget`] -> default-full-lib-file-name map.
// Go: internal/tsoptions/enummaps.go:targetToLibMap / TargetToLibMap
pub fn target_to_lib_map(target: ScriptTarget) -> Option<&'static str> {
    match target {
        ScriptTarget::EsNext => Some("lib.esnext.full.d.ts"),
        ScriptTarget::Es2025 => Some("lib.es2025.full.d.ts"),
        ScriptTarget::Es2024 => Some("lib.es2024.full.d.ts"),
        ScriptTarget::Es2023 => Some("lib.es2023.full.d.ts"),
        ScriptTarget::Es2022 => Some("lib.es2022.full.d.ts"),
        ScriptTarget::Es2021 => Some("lib.es2021.full.d.ts"),
        ScriptTarget::Es2020 => Some("lib.es2020.full.d.ts"),
        ScriptTarget::Es2019 => Some("lib.es2019.full.d.ts"),
        ScriptTarget::Es2018 => Some("lib.es2018.full.d.ts"),
        ScriptTarget::Es2017 => Some("lib.es2017.full.d.ts"),
        ScriptTarget::Es2016 => Some("lib.es2016.full.d.ts"),
        // We don't use lib.es2015.full.d.ts due to a breaking change.
        ScriptTarget::Es2015 => Some("lib.es6.d.ts"),
        _ => None,
    }
}

/// Returns the default library file name implied by the effective emit target,
/// or `"lib.d.ts"` when the target is not in [`target_to_lib_map`].
///
/// # Examples
/// ```
/// use tsgo_tsoptions::get_default_lib_file_name;
/// use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
/// let mut o = CompilerOptions::default();
/// o.target = ScriptTarget::Es2020;
/// assert_eq!(get_default_lib_file_name(&o), "lib.es2020.full.d.ts");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/enummaps.go:GetDefaultLibFileName
pub fn get_default_lib_file_name(options: &CompilerOptions) -> String {
    match target_to_lib_map(options.get_emit_script_target()) {
        Some(name) => name.to_string(),
        None => "lib.d.ts".to_string(),
    }
}

#[cfg(test)]
#[path = "enummaps_test.rs"]
mod tests;
