# modulespecifiers: 实现方案（impl.md）

**crate**：`tsgo_modulespecifiers`　**目标**：模块解析的"反方向"——给定一个目标文件（要导入的符号所在文件）与导入文件，**生成最合适的模块说明符字符串**（相对路径 / `paths` 别名 / `node_modules` 包名 / `exports` 子路径 / ambient module），并按用户偏好（relative/non-relative/shortest、扩展名 ending）排序选优。供自动导入（auto-import）、quick-fix、`.d.ts` emit 用。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_debug` `tsgo_module` `tsgo_packagejson` `tsgo_symlinks` `tsgo_tsoptions` `tsgo_tspath`
**Go 源**：`internal/modulespecifiers/`（5 个非测试文件，约 3 万字符：`specifiers.go` ~1400 行 + `util.go` ~430 行 + `preferences.go` ~250 行 + `types.go` + `compare.go`）

## 这个包是什么（业务说明）

当语言服务要给你自动补全一个未导入的符号时，它得**凭空造出一行 import**——但"造哪种 import"有很多选择：写相对路径 `../lib/utils`？写包名 `lodash`？走 `paths` 别名 `@app/utils`？带不带 `.js`/`.ts` 扩展名？要不要走 symlink 路径？这个包就是这套"说明符生成 + 偏好排序 + 去重"的算法。

它和 `module`（解析器）是镜像关系：`module` 是 specifier→file，`modulespecifiers` 是 file→specifier。两者共享大量规则（`exports`/`imports` 通配匹配、`node_modules` 路径解析、symlink），所以本包 import 了 `module`、`symlinks`、`packagejson`。生成逻辑核心：
- `getAllModulePaths`：找出目标文件的所有等价路径（含 symlink、redirect）。
- `computeModuleSpecifiers`：对每条路径，尝试 `paths` / `node_modules` 包名 / `exports` / 相对路径几种生成方式。
- `preferences.go`：把 `UserPreferences` + `compilerOptions` 翻译成 `relativePreference` + `allowedEndings`（扩展名优先序）。

它在 Phase 4 因为依赖 `module`/`symlinks`/`packagejson`，且被 checker import（`Host = ModuleSpecifierGenerationHost`）。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包关键决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `ModuleSpecifierGenerationHost interface`（~13 方法） | `pub trait ModuleSpecifierGenerationHost` | 行为接口 → trait。返回的 `*module.ResolvedModule`/`*packagejson.InfoCacheEntry` 用引用/`Option<Arc<...>>` |
| `SourceFileForSpecifierGeneration` / `CheckerShape` interface | 对应 trait | `CheckerShape` 是 checker 注入的最小接口（`GetSymbolAtLocation`/`GetAliasedSymbol`），打破对 checker 的依赖 |
| `ImportModuleSpecifierPreference string` 等"字符串枚举" | `enum ...Preference`（带 `as_str()`/`from_str()`） | Go 用带值 `string` 常量（`"shortest"` 等）；Rust 用判别枚举，序列化时映射回字符串 |
| `RelativePreferenceKind`/`ModuleSpecifierEnding`/`MatchingMode`/`ResultKind` uint8 iota | `#[repr(u8)] enum` | 普通枚举（PORTING §3） |
| `ModulePath{ FileName; IsInNodeModules; IsRedirect }` | 同名 struct | |
| `ModuleSpecifierPreferences{ relativePreference; getAllowedEndingsInPreferredOrder func(...); excludeRegexes }` | `struct ModuleSpecifierPreferences { relative_preference, get_allowed_endings: Box<dyn Fn(ResolutionMode)->Vec<ModuleSpecifierEnding>>, exclude_regexes }` | 内含闭包成员；Rust 用 `Box<dyn Fn>` 或重构成方法 + 捕获字段 |
| `stringToRegex` 缓存的正则 | `regex::Regex`（用 `OnceCell`/惰性缓存映射 pattern→Regex） | `IsExcludedByRegex` 用 `AutoImportSpecifierExcludeRegexes` |
| `NodeModulePathParts` | 同名 struct | `GetNodeModulePathParts` 解析返回 |

**无 arena 自有所有权**：本包通过 host trait 读外部状态（symlink cache、package.json info、resolved module），自身不持有长生命周期图。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/modulespecifiers/specifiers.go` | `internal/modulespecifiers/lib.rs` | crate 根。`GetModuleSpecifiers[WithInfo]`、`getAllModulePaths[Worker]`、`computeModuleSpecifiers`、`getLocalModuleSpecifier`、`tryGetModuleNameAsNodeModule`、`tryGetModuleNameFrom{Exports,Imports,Paths,RootDirs,ExportsOrImports,AmbientModule}`、`GetEachFileNameOfModule`、`ContainsNodeModules`、`containsIgnoredPath`、`processEnding`、`UpdateModuleSpecifier` 等 |
| `internal/modulespecifiers/preferences.go` | `internal/modulespecifiers/preferences.rs` | `ModuleSpecifierPreferences`、`GetAllowedEndingsInPreferredOrder`、`getPreferredEnding`、`getModuleSpecifierEndingPreference`、`inferPreference`、`usesExtensionsOnImports`、`shouldAllowImportingTsExtension` |
| `internal/modulespecifiers/util.go` | `internal/modulespecifiers/util.rs` | `TryGetRealFileNameForNonJSDeclarationFileName`、`GetNodeModulePathParts`、`GetNodeModulesPackageName`、`GetPackageNameFromDirectory`、`IsExcludedByRegex`、`stringToRegex`、`ProcessEntrypointEnding`、`ensurePathIsNonModuleName`、`prefersTsExtension` 等 |
| `internal/modulespecifiers/types.go` | `internal/modulespecifiers/types.rs` | 所有 trait + 枚举 + `ModulePath`/`UserPreferences`/`ModuleSpecifierOptions` |
| `internal/modulespecifiers/compare.go` | `internal/modulespecifiers/compare.rs` | `CountPathComponents` |

## 依赖白名单（本包新增的 crate）

- `regex`（`AutoImportSpecifierExcludeRegexes`）——记 `crate-map.md`。
- 其余依赖均 workspace 内。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `compare.rs`（Go: `compare.go`）

- [ ] `pub fn count_path_components(path: &str) -> usize`（去掉前导 `./` 后数 `/`）　`// Go: compare.go:CountPathComponents`

### `types.rs`（Go: `types.go`）

- [ ] `pub trait SourceFileForSpecifierGeneration { fn path; fn file_name; fn imports; fn is_js }`　`// Go: types.go:SourceFileForSpecifierGeneration`
- [ ] `pub trait CheckerShape { fn get_symbol_at_location; fn get_aliased_symbol }`　`// Go: types.go:CheckerShape`
- [ ] `pub trait ModuleSpecifierGenerationHost`（13 方法）　`// Go: types.go:ModuleSpecifierGenerationHost`
- [ ] `#[repr(u8)] enum ResultKind { None, NodeModules, Paths, Redirect, Relative, Ambient }`　`// Go: types.go:ResultKind`
- [ ] `pub struct ModulePath { file_name, is_in_node_modules, is_redirect }`　`// Go: types.go:ModulePath`
- [ ] `enum ImportModuleSpecifierPreference`（None/Shortest/ProjectRelative/Relative/NonRelative）+ `as_str`　`// Go: types.go`
- [ ] `enum ImportModuleSpecifierEndingPreference`（None/Auto/Minimal/Index/Js）　`// Go: types.go`
- [ ] `pub struct UserPreferences { import_module_specifier_preference, import_module_specifier_ending, auto_import_specifier_exclude_regexes }`　`// Go: types.go:UserPreferences`
- [ ] `pub struct ModuleSpecifierOptions { override_import_mode: ResolutionMode }`　`// Go: types.go:ModuleSpecifierOptions`
- [ ] `enum RelativePreferenceKind { Relative, NonRelative, Shortest, ExternalNonRelative }`　`// Go: types.go`
- [ ] `enum ModuleSpecifierEnding { Minimal, Index, JsExtension, TsExtension }`　`// Go: types.go`
- [ ] `enum MatchingMode { Exact, Directory, Pattern }`　`// Go: types.go`

### `preferences.rs`（Go: `preferences.go`）

- [ ] `fn should_allow_importing_ts_extension(options, from_file_name) -> bool`　`// Go: preferences.go:shouldAllowImportingTsExtension`
- [ ] `fn uses_extensions_on_imports(file) -> bool`　`// Go: preferences.go:usesExtensionsOnImports`
- [ ] `fn infer_preference(resolution_mode, source_file, module_resolution_is_nodenext) -> ModuleSpecifierEnding`　`// Go: preferences.go:inferPreference`
- [ ] `fn get_module_specifier_ending_preference(pref, resolution_mode, options, source_file) -> ModuleSpecifierEnding`　`// Go: preferences.go:getModuleSpecifierEndingPreference`
- [ ] `fn get_preferred_ending(prefs, host, options, importing_file, old_specifier, resolution_mode) -> ModuleSpecifierEnding`　`// Go: preferences.go:getPreferredEnding`
- [ ] `pub struct ModuleSpecifierPreferences { relative_preference, get_allowed_endings, exclude_regexes }`　`// Go: preferences.go:ModuleSpecifierPreferences`
- [ ] `pub fn get_allowed_endings_in_preferred_order(...) -> Vec<ModuleSpecifierEnding>`（按 preferredEnding × allowImportingTsExtension × nodenext-ESM 的分支矩阵；`debug::assert_never` 兜底）　`// Go: preferences.go:GetAllowedEndingsInPreferredOrder`
- [ ] `fn get_module_specifier_preferences(prefs, host, options, importing_file, old_specifier) -> ModuleSpecifierPreferences`　`// Go: preferences.go:getModuleSpecifierPreferences`

### `util.rs`（Go: `util.go`）

- [ ] `fn compare_paths_by_redirect`　`// Go: util.go:comparePathsByRedirect`
- [ ] `pub fn path_is_bare_specifier(path) -> bool`　`// Go: util.go:PathIsBareSpecifier`
- [ ] `pub fn is_excluded_by_regex(specifier, excludes) -> bool` + `fn string_to_regex(pattern) -> Option<Regex>`（带缓存）　`// Go: util.go:IsExcludedByRegex/stringToRegex`
- [ ] `fn ensure_path_is_non_module_name(path) -> String`　`// Go: util.go:ensurePathIsNonModuleName`
- [ ] `pub fn get_js_extension_for_declaration_file_extension(ext) -> String`　`// Go: util.go:GetJSExtensionForDeclarationFileExtension`
- [ ] `pub fn try_get_real_file_name_for_non_js_declaration_file_name(file_name) -> String`（`.d.json.ts`→`.json`、`.module.d.css.ts`→`.module.css`、纯 `.d.ts`→`""`）　`// Go: util.go:TryGetRealFileNameForNonJSDeclarationFileName`
- [ ] `fn get_js_extension_for_file` / `extension_from_path` / `try_get_any_file_from_path`　`// Go: util.go`
- [ ] `fn get_paths_relative_to_root_dirs` / `is_path_relative_to_parent` / `get_relative_path_if_in_same_volume` / `package_json_paths_are_equal` / `prefers_ts_extension`　`// Go: util.go`
- [ ] `fn replace_first_star(s, replacement) -> String`　`// Go: util.go:replaceFirstStar`
- [ ] `pub struct NodeModulePathParts{...}` + `pub fn get_node_module_path_parts(full_path) -> Option<NodeModulePathParts>`　`// Go: util.go:GetNodeModulePathParts`
- [ ] `pub fn get_node_modules_package_name(...) -> String`　`// Go: util.go:GetNodeModulesPackageName`
- [ ] `fn all_keys_start_with_dot(obj) -> bool`　`// Go: util.go:allKeysStartWithDot`
- [ ] `pub fn get_package_name_from_directory(path) -> String`　`// Go: util.go:GetPackageNameFromDirectory`
- [ ] `pub fn process_entrypoint_ending(...)`　`// Go: util.go:ProcessEntrypointEnding`

### `lib.rs`（Go: `specifiers.go`，按子区块勾选）

- [ ] `pub fn get_module_specifiers(...) -> Vec<String>`　`// Go: specifiers.go:GetModuleSpecifiers`
- [ ] `pub fn get_module_specifiers_with_info(...)`　`// Go: specifiers.go:GetModuleSpecifiersWithInfo`
- [ ] `pub fn get_module_specifiers_for_file_with_info(...)`　`// Go: specifiers.go:GetModuleSpecifiersForFileWithInfo`
- [ ] `fn try_get_module_name_from_ambient_module(module_symbol, checker) -> String`　`// Go: specifiers.go:tryGetModuleNameFromAmbientModule`
- [ ] `fn get_info(...)` / `fn get_all_module_paths[_worker](...)`　`// Go: specifiers.go:getAllModulePaths`
- [ ] `fn contains_ignored_path(s) -> bool` / `pub fn contains_node_modules(s) -> bool`　`// Go: specifiers.go:containsIgnoredPath/ContainsNodeModules`
- [ ] `pub fn get_each_file_name_of_module(importing_file, imported_file, host, prefer_symlinks) -> Vec<ModulePath>`（含 symlink 展开 + ignored-path 兜底"至少返回 1 条"）　`// Go: specifiers.go:GetEachFileNameOfModule`
- [ ] `fn compute_module_specifiers(...)`（核心：对每条 ModulePath 试 paths/node_modules/exports/relative）　`// Go: specifiers.go:computeModuleSpecifiers`
- [ ] `fn get_local_module_specifier(...)`（相对/baseUrl 本地说明符）　`// Go: specifiers.go:getLocalModuleSpecifier`
- [ ] `fn process_ending(...)`　`// Go: specifiers.go:processEnding`
- [ ] `fn try_get_module_name_from_root_dirs(...)`　`// Go: specifiers.go:tryGetModuleNameFromRootDirs`
- [ ] `fn try_get_module_name_as_node_module(...)` + `fn try_directory_with_package_json(...)`　`// Go: specifiers.go:tryGetModuleNameAsNodeModule`
- [ ] `fn try_get_module_name_from_exports(...)` / `from_package_json_imports(...)`　`// Go: specifiers.go:tryGetModuleNameFromExports/...Imports`
- [ ] `fn try_get_module_name_from_paths(...)` + `fn validate_ending(...)`　`// Go: specifiers.go:tryGetModuleNameFromPaths`
- [ ] `fn try_get_module_name_from_exports_or_imports(options, host, target_file_path, package_dir, key, target, conditions, mode, is_imports, is_pattern) -> String`（通配 `*` 匹配 + trailer）　`// Go: specifiers.go:tryGetModuleNameFromExportsOrImports`
- [ ] `pub fn get_module_specifier(...)` / `pub fn update_module_specifier(...)` / `fn get_module_specifier_with_preferences(...)`　`// Go: specifiers.go:GetModuleSpecifier/UpdateModuleSpecifier`

### Cargo / crate 接线

- [ ] `internal/modulespecifiers/Cargo.toml`（`name = "tsgo_modulespecifiers"` + path deps）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明 `mod types; mod preferences; mod util; mod compare;` + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `compare.rs::count_path_components` + `types.rs` 枚举（位/repr 快照）。
2. `util.rs` 纯函数（`TryGetRealFileNameForNonJSDeclarationFileName`、`GetNodeModulePathParts`、`IsExcludedByRegex`）→ 对应 `TestTryGetRealFileNameForNonJSDeclarationFileName`。
3. `specifiers.rs::contains_node_modules`/`contains_ignored_path` → `TestContainsNodeModules`/`TestContainsIgnoredPath`。
4. `GetEachFileNameOfModule`（用 mock host + symlink cache）→ `TestGetEachFileNameOfModule[WithSymlinks]`。
5. `tryGetModuleNameFromExportsOrImports`（通配匹配）→ `TestTryGetModuleNameFromExportsOrImports`。
6. `preferences.rs` 偏好矩阵（无直接单测，靠 conformance；补行为级）。
7. `computeModuleSpecifiers` 全链路（主要靠 P10 auto-import parity）。

## 与 Go 的已知偏离（divergence）

- 字符串枚举（`"shortest"` 等）→ Rust 判别枚举 + `as_str`/`from_str`。
- `ModuleSpecifierPreferences.getAllowedEndingsInPreferredOrder` 闭包成员 → `Box<dyn Fn>`（或重构为持字段的方法）。
- `stringToRegex` 正则缓存 → `regex::Regex` + 惰性 map。
- host 返回的 `*module.ResolvedModule`/`*packagejson.InfoCacheEntry` 指针 → 引用/`Option<Arc>`。

## 转交 / 推迟（DEFER）

- `computeModuleSpecifiers` / `getLocalModuleSpecifier` / preferences 的**全量正确性**靠 P10 auto-import / `.d.ts` emit parity 兜底（Go 侧单测只覆盖了 6 个点）。
- 依赖 `tsgo_tsoptions`（P6）的 `SourceOutputAndProjectReference` —— **注意**：`tsoptions` 在 README 里属 P6，但本包（P4）已 import 它。这是一处**跨 phase 依赖倒挂**，见"存疑偏离"（README 与 phase 文档需协调：要么 `tsoptions` 的被依赖子集提前到 P4，要么本包对 project-reference 相关入口标 `// DEFER(phase-6) / blocked-by: tsgo_tsoptions`）。
