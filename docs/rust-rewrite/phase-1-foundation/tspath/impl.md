# tspath: 实现方案（impl.md）

**crate**：`tsgo_tspath`　**目标**：TypeScript 的路径模型与全部路径工具（统一以 `/` 为分隔符的规范化、根长度计算、拼接、相对路径、扩展名处理、大小写规范化等）。
**依赖（crate）**：`tsgo_stringutil`（比较器/大小写）。
**Go 源**：`internal/tspath/`（3 个非测试文件：`path.go` 1220、`extension.go` 199、`ignoredpaths.go` 18 行）

## 这个包是什么（业务说明）

`tspath` 是编译器的"路径中枢"。TS 内部一律用 `/` 作为目录分隔符，并把文件名规范化为一个 `Path`（已 rooted、已 reduce、已按大小写规则 canonical 化的字符串），作为各种 map 的 key。本包被 module resolution、program、vfs、ls 等大量依赖，移植正确性极其关键（路径错一点，整个解析就崩）。

核心概念：
- **`Path` 类型**：`type Path string`，代表"规范化 + canonical 化"的路径，用作缓存键。
- **根长度（root length）**：`GetEncodedRootLength` 统一处理 POSIX(`/`)、UNC(`//server/`)、DOS(`c:/`)、untitled(`^/`)、URL(`file://`/`http://`) 五类根，URL 用按位取反（`^x`）编码以区分"是 URL"。`GetRootLength` 解码。
- **规范化**：`NormalizeSlashes`（`\`→`/`）、`GetNormalizedAbsolutePath`（手写高性能 normalizer，处理 `.`/`..`/多余 `/`）、`NormalizePath`、`simpleNormalizePath`（快路径）、`hasRelativePathSegment`（手写检测 `.`/`..`/`//` 段，替代正则）。
- **大小写**：`ToFileNameLowerCase`（含 `\u0130` 特例的小写化）、`GetCanonicalFileName`。
- **相对路径 / 比较 / 公共父目录**：`GetRelativePathToDirectoryOrUrl`、`ComparePaths`、`GetCommonParents`、`ContainsPath`、`StartsWithDirectory`。
- **扩展名**（`extension.go`）：全部 TS/JS 扩展名常量 + 增删改判定。
- **忽略路径**（`ignoredpaths.go`）：`node_modules/.`、`.git`、`.#` 的子串检测。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type Path string` | `pub struct Path(String)`（newtype） + `&PathRef`/`&str` 借用 | 用 newtype 区分"规范化路径"与普通字符串，避免误用。**注意**：不是 `std::path::Path`（那是 OS 路径，含平台分隔符），这是 TS 自己的 `/`-路径模型 |
| `GetEncodedRootLength` 的 `^x`（按位取反编码 URL） | `enum RootLength { Disk(usize), Url(usize) }` 或返回 `i32` 保留位运算 | Go 用负数（`^x`）表示"这是 URL 根"。Rust 可保留 `i32` + `!x` 直译，或更清晰地用枚举。**优先保留 i32 直译**以减少调用点改动，存疑处加注释 |
| `int` 长度/偏移 | `usize` / `i32` | 根长度计算大量用 `int`；URL 分支返回负数，故内部用 `i32`，对外 `GetRootLength` 返回 `usize` |
| `strings.*`（Split/Index/HasPrefix...） | `str::split` / `find` / `starts_with` 等 | 直译；注意字节索引语义（Go 按字节，Rust `&str` 切片也按字节边界，需保证落在 UTF-8 边界） |
| `func(a,b string) int`（comparer） | `fn(&str,&str)->Ordering` | 来自 `tsgo_stringutil` |
| `ComparePathsOptions{UseCaseSensitiveFileNames, CurrentDirectory}` | `struct ComparePathsOptions{ use_case_sensitive_file_names: bool, current_directory: String }` | 配置结构 |
| `ForEachAncestorDirectory[T](dir, cb) (T, bool)`（泛型 + 回调） | `fn for_each_ancestor_directory<T>(dir, cb: impl FnMut(&str)->ControlFlow<T>) -> Option<T>` | 泛型回调；`stop bool` → `ControlFlow`/`Option` |
| `unsafe.String(&b[0], len)`（`ToFileNameLowerCase` 零拷贝） | `String::from_utf8(b).unwrap()` / 直接 `String` | **去 unsafe**：Rust 直接构造 `String`（PORTING §0 要零 unsafe）；标 `// PERF(port)` |
| `GetCommonParents` 用 reflect-free 泛型 `getPathComponents` 参数 | 泛型/`fn` 指针参数 | 直译 |
| 包级 `var Supported*Extensions [][]string` | `static`/`const` 数组（`once_cell`/`LazyLock` 拼接的用 `LazyLock`） | `slices.Concat` 拼接的用 `LazyLock<Vec<...>>` |

> **关键正确性点**：`GetEncodedRootLength` 的五类根 + URL 取反编码是本包的命门。`GetNormalizedAbsolutePath` 是手写优化版（有大量 fuzz 测试对拍旧实现 `getNormalizedAbsolutePath_old`），移植时必须逐分支照搬并用 Go 实测值做 gate。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/tspath/path.go` | `internal/tspath/path.rs`（在 `lib.rs` 里 `mod path; pub use path::*;`） | Path 类型 + 全部路径工具 |
| `internal/tspath/extension.go` | `internal/tspath/extension.rs` | 扩展名常量 + 判定/增删改 |
| `internal/tspath/ignoredpaths.go` | `internal/tspath/ignoredpaths.rs` | `ContainsIgnoredPath` |
| （crate 根） | `internal/tspath/lib.rs` | 声明子模块 + re-export |

## 依赖白名单（本包新增的 crate）

- 无新增外部 crate（`LazyLock` 用标准库；正则被手写函数替代，无需 `regex`）。
- path 依赖 `tsgo_stringutil`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `path.rs`（Go: `internal/tspath/path.go`）

判定族：
- [ ] `pub struct Path(String)` + `const DIRECTORY_SEPARATOR = '/'`　`// Go: path.go:Path/DirectorySeparator`
- [ ] `is_any_directory_separator(byte) -> bool`（私有，`/` 或 `\`）　`// Go: path.go:isAnyDirectorySeparator`
- [ ] `is_url / is_rooted_disk_path / is_disk_path_root / is_dynamic_file_name / path_is_absolute / has_trailing_directory_separator`　`// Go: path.go:IsUrl/IsRootedDiskPath/IsDiskPathRoot/IsDynamicFileName/PathIsAbsolute/HasTrailingDirectorySeparator`
- [ ] `is_volume_character(byte) -> bool`　`// Go: path.go:IsVolumeCharacter`

根长度（命门）：
- [ ] `get_encoded_root_length(path) -> i32` — POSIX/UNC/DOS/untitled(`^/`)/URL(`file://`/`http://`)；URL 用 `!x` 编码；`file://(localhost)` 含 DOS 卷的特例（`c:`/`c%3a`/`c%3A`）　`// Go: path.go:GetEncodedRootLength`
- [ ] `get_file_url_volume_separator_end(url, start) -> i32`（私有；`:`/`%3a`/`%3A`）　`// Go: path.go:getFileUrlVolumeSeparatorEnd`
- [ ] `get_root_length(path) -> usize` — 解码（负→`!x`）　`// Go: path.go:GetRootLength`

拼接 / 组件：
- [ ] `combine_paths(first, paths...) -> String` — 绝对路径覆盖前序；不简化相对段；预分配　`// Go: path.go:CombinePaths`
- [ ] `get_path_components(path, current_directory) -> Vec<String>`　`// Go: path.go:GetPathComponents`
- [ ] `path_components(path, root_length) -> Vec<String>`（私有）　`// Go: path.go:pathComponents`
- [ ] `reduce_path_components(components) -> Vec<String>`（私有；消 `.`/`..`/空段）　`// Go: path.go:reducePathComponents`
- [ ] `get_path_from_path_components(components) -> String`　`// Go: path.go:GetPathFromPathComponents`
- [ ] `get_normalized_path_components(path, current_directory) -> Vec<String>` + `get_normalized_path_components_from_combined`　`// Go: path.go:GetNormalizedPathComponents/getNormalizedPathComponentsFromCombined`

规范化：
- [ ] `normalize_slashes(path) -> String`（`\`→`/`）　`// Go: path.go:NormalizeSlashes`
- [ ] `simple_normalize_path(path) -> Option<String>`（快路径）　`// Go: path.go:simpleNormalizePath`
- [ ] `has_relative_path_segment(p) -> bool`（手写检测 `.`/`..`/`//`/`/./`/`/../`，替代正则）　`// Go: path.go:hasRelativePathSegment`
- [ ] `get_normalized_absolute_path(file_name, current_directory) -> String`（手写 normalizer，逐分支照搬）　`// Go: path.go:GetNormalizedAbsolutePath`
- [ ] `get_normalized_absolute_path_without_root(file_name, current_directory) -> String`　`// Go: path.go:GetNormalizedAbsolutePathWithoutRoot`
- [ ] `normalize_path(path) -> String`　`// Go: path.go:NormalizePath`
- [ ] `resolve_path(path, paths...) -> String` / `resolve_tripleslash_reference(module_name, containing_file)`　`// Go: path.go:ResolvePath/ResolveTripleslashReference`

大小写 / Path 构造：
- [ ] `to_file_name_lower_case(file_name) -> String`（ASCII 快路径 + `\u0130` 特例；去 unsafe）　`// Go: path.go:ToFileNameLowerCase`
- [ ] `get_canonical_file_name(file_name, use_case_sensitive_file_names) -> String`　`// Go: path.go:GetCanonicalFileName`
- [ ] `to_path(file_name, base_path, use_case_sensitive_file_names) -> Path`　`// Go: path.go:ToPath`

目录 / 分隔符：
- [ ] `get_directory_path(path) -> String` + `Path::get_directory_path`　`// Go: path.go:GetDirectoryPath`
- [ ] `remove_trailing_directory_separator(s)` / `remove_trailing_directory_separators(s)` / `ensure_trailing_directory_separator(s)` + Path 方法　`// Go: path.go:Remove*/Ensure*`
- [ ] `get_base_file_name(path) -> String`　`// Go: path.go:GetBaseFileName`

相对路径 / 比较：
- [ ] `get_path_components_relative_to(from, to, options) -> Vec<String>`　`// Go: path.go:GetPathComponentsRelativeTo`
- [ ] `get_relative_path_from_directory / get_relative_path_from_file / convert_to_relative_path / get_relative_path_to_directory_or_url`　`// Go: path.go:GetRelativePath*/ConvertToRelativePath`
- [ ] `struct ComparePathsOptions` + `get_comparer / get_equality_comparer`　`// Go: path.go:ComparePathsOptions`
- [ ] `compare_paths / compare_paths_case_sensitive / compare_paths_case_insensitive`　`// Go: path.go:ComparePaths*`
- [ ] `contains_path(parent, child, options) -> bool` + `Path::contains_path(child) -> bool`（前缀检查）　`// Go: path.go:ContainsPath`
- [ ] `compare_number_of_directory_separators(p1, p2) -> Ordering`　`// Go: path.go:CompareNumberOfDirectorySeparators`

扩展名 / 模块名 / 祖先目录：
- [ ] `get_any_extension_from_path(path, extensions, ignore_case) -> String`（+ workers）　`// Go: path.go:GetAnyExtensionFromPath/getAnyExtensionFromPathWorker/tryGetExtensionFromPath`
- [ ] `file_extension_is(path, ext) -> bool` / `has_extension(file_name) -> bool`　`// Go: path.go:FileExtensionIs/HasExtension`
- [ ] `path_is_relative / ensure_path_is_non_module_name / is_external_module_name_relative`　`// Go: path.go:PathIsRelative/EnsurePathIsNonModuleName/IsExternalModuleNameRelative`
- [ ] `split_volume_path(path) -> Option<(String, &str)>`　`// Go: path.go:SplitVolumePath`
- [ ] `for_each_ancestor_directory<T>(dir, cb) -> Option<T>` / `..._stopping_at_global_cache` / `..._path`（泛型 + ControlFlow）　`// Go: path.go:ForEachAncestorDirectory*`
- [ ] `get_common_parents(paths, min_components, get_path_components, options) -> (Vec<String>, HashSet<String>)` + `get_common_parents_worker`（递归 fan-out）　`// Go: path.go:GetCommonParents/getCommonParentsWorker`
- [ ] `starts_with_directory(file_name, directory_name, use_case_sensitive_file_names) -> bool`　`// Go: path.go:StartsWithDirectory`

### `extension.rs`（Go: `internal/tspath/extension.go`）

- [ ] 扩展名常量 `EXTENSION_TS=".ts"` ... `EXTENSION_DCTS=".d.cts"`（13 个）　`// Go: extension.go`（const 块）
- [ ] `static` 扩展名集合：`SUPPORTED_DECLARATION_EXTENSIONS` / `SUPPORTED_TS_IMPLEMENTATION_EXTENSIONS` / `ALL_SUPPORTED_EXTENSIONS` / `SUPPORTED_TS_EXTENSIONS(_FLAT/_WITH_JSON)` / `SUPPORTED_JS_EXTENSIONS(_FLAT)` / `EXTENSIONS_NOT_SUPPORTING_EXTENSIONLESS_RESOLUTION` 等（拼接的用 `LazyLock`）　`// Go: extension.go`（var 块）
- [ ] `extension_is_ts(ext) -> bool`（含 `.d.*.ts` 判定）　`// Go: extension.go:ExtensionIsTs`
- [ ] `remove_file_extension / try_get_extension_from_path / remove_extension / file_extension_is_one_of / try_extract_ts_extension`　`// Go: extension.go:*`
- [ ] `has_ts_file_extension / has_implementation_ts_file_extension / has_js_file_extension / has_json_file_extension`　`// Go: extension.go:*`
- [ ] `is_declaration_file_name / extension_is_one_of / get_declaration_file_extension / get_declaration_emit_extension_for_path`　`// Go: extension.go:*`
- [ ] `change_any_extension / change_extension / change_full_extension`　`// Go: extension.go:*`
- [ ] `get_possible_original_input_extension_for_extension(path) -> Vec<String>`　`// Go: extension.go:GetPossibleOriginalInputExtensionForExtension`

### `ignoredpaths.rs`（Go: `internal/tspath/ignoredpaths.go`）

- [ ] `static IGNORED_PATHS = ["/node_modules/.", "/.git", ".#"]`　`// Go: ignoredpaths.go:ignoredPaths`
- [ ] `contains_ignored_path(path) -> bool`（任一子串命中）　`// Go: ignoredpaths.go:ContainsIgnoredPath`

### Cargo / crate 接线

- [ ] `internal/tspath/Cargo.toml`（`name = "tsgo_tspath"`，dep `tsgo_stringutil` path）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明 `mod path; mod extension; mod ignoredpaths;` + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `normalize_slashes` + `get_encoded_root_length` / `get_root_length`（命门；`TestGetRootLength` 全 50+ 断言 + `TestUntitledPath*` 做 gate）。
2. 判定族 `is_url`/`is_rooted_disk_path`/`path_is_absolute`（`TestIsUrl`/`TestIsRootedDiskPath`/`TestPathIsAbsolute`）。
3. `combine_paths` + `get_path_components` + `reduce_path_components`（`TestCombinePaths`/`TestGetPathComponents`/`TestReducePathComponents`）。
4. `get_directory_path` / `get_base_file_name`（`TestGetDirectoryPath`）。
5. `has_relative_path_segment` + `get_normalized_absolute_path` + `normalize_path` + `resolve_path`（最难；`TestGetNormalizedAbsolutePath` ~100 断言 + Fuzz 对拍旧实现）。
6. `to_file_name_lower_case` / `to_path`（`TestToFileNameLowerCase`/`TestToPath`）。
7. 相对路径 + `compare_paths` + `get_common_parents` + `starts_with_directory`（`TestGetRelativePathToDirectoryOrUrl`/`TestGetCommonParents`/`TestStartsWithDirectory*`）。
8. `extension.rs` 全族 + `contains_ignored_path`（`TestContainsIgnoredPath`/`TestIgnoredPaths*`）。

## 与 Go 的已知偏离（divergence）

- **URL 根的负数编码**：Go `GetEncodedRootLength` 用 `^x`（负数）标记 URL 根。Rust 优先保留 `i32` + `!x` 直译以减少调用点改动；可选用 `enum RootLength` 更清晰，但需改所有调用点。本包选 `i32` 直译，注释说明。
- **去 unsafe**：`ToFileNameLowerCase` 的 `unsafe.String` 零拷贝改为安全 `String` 构造（`// PERF(port)`，PORTING 要零 unsafe）。
- **正则 → 手写**：`hasRelativePathSegment` / `ToFileNameLowerCase` 在 Go 已是手写函数（测试里 `old*` 是正则版做 fuzz 对拍）。Rust 照搬手写版，**不**引入 `regex`。
- **`Path` newtype vs `std::path::Path`**：本包的 `Path` 是 TS 的 `/`-字符串模型，**绝不**用 `std::path::Path`（会引入平台分隔符）。所有 OS 交互在 vfs 层转换。
- **字节索引**：Go 大量按字节切片路径。Rust `&str` 切片要求落在 UTF-8 边界；ASCII 路径分隔符运算安全，含多字节字符的相对/比较路径需注意（测试含 unicode 用例 `测试`，须用字节/字符一致的切法）。
- **泛型回调 + ControlFlow**：`ForEachAncestorDirectory` 的 `(result, stop)` → `ControlFlow`/`Option`，结构等价。

## 转交 / 推迟（DEFER）

- `GetCommonParents` 被 program/project 使用；本包完整实现并测，但其真实集成对拍归 P10。
- Fuzz 测试（`FuzzGetNormalizedAbsolutePath` 等对拍 `*_old`）转为 Rust property test（`proptest`/`quickcheck`），列入 tests.md 推迟表，实现期补。
