# module: 实现方案（impl.md）

**crate**：`tsgo_module`　**目标**：Node/TS 的**模块解析器**——把一个模块说明符（`"./x"`、`"pkg"`、`"@scope/pkg/sub"`、type reference directive）在给定 `compilerOptions` 与 vfs 下解析到具体磁盘文件，处理 `package.json` 的 `exports`/`imports`/`typesVersions`/`main`/`types`、`paths`、`rootDirs`、`typeRoots`、symlink、node_modules 逐级上溯，并产出解析诊断 trace。
**依赖（crate）**：`tsgo_ast` `tsgo_collections` `tsgo_core` `tsgo_diagnostics` `tsgo_packagejson` `tsgo_semver` `tsgo_stringutil` `tsgo_tspath` `tsgo_vfs`（含 `vfsmatch`）
**Go 源**：`internal/module/`（4 个非测试文件，约 2700 行：`resolver.go` ~2300 行是主体）

## 这个包是什么（业务说明）

`module` 是编译器最复杂的"纯算法"子系统之一——它把 Node.js / TypeScript 那套庞杂的模块解析规则（CommonJS vs ESM、`node16`/`nodenext`/`bundler` 三套 feature 集、`package.json` 的 `exports`/`imports` 条件匹配与通配 trailer、`typesVersions`、`@types/` 回退、`paths` 通配、`rootDirs`、symlink realpath、`.ts`↔`.js` 扩展名映射）完整实现一遍。结果是 `ResolvedModule` / `ResolvedTypeReferenceDirective`（解析到的文件 + 扩展名 + PackageId + 诊断）。

它被 checker（解析 import）、program、`modulespecifiers`（反向：给定文件生成说明符时要复用同样的规则）依赖。它**不读源码内容**，只做路径/JSON/文件存在性判定（通过 `ResolutionHost.FS()`），是 CPU + vfs I/O bound 的纯算法包。

并发要点：`Resolver` 内含多个 `SyncMap` 缓存（module 解析缓存、typeRef 缓存、`package.json` info 缓存），program 会从多个 goroutine 并发解析。回归测试 `TestResolveModuleNameTrailingSlashRace` 专门盯 `package.json` info-cache 的插入竞态——移植时必须保持等价的 check→`LoadOrStore` 语义。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5/§6。本包关键决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type ResolutionHost interface { FS() vfs.FS; GetCurrentDirectory() string }` | `pub trait ResolutionHost { fn fs(&self) -> &dyn Vfs; fn get_current_directory(&self) -> &str }` | 行为接口 → trait |
| `ResolvedModule` / `ResolvedTypeReferenceDirective`（多字段返回） | 同名 `struct`（`pub`），`IsResolved()` → `is_resolved()` | `[]*ast.Diagnostic` → `Vec<ast::Diagnostic>`（或 `DiagAndArgs`，见下） |
| `resolved`（包内私有解析中间态，`nil`=继续找） | `enum Resolved { ContinueSearching, Unresolved, Found(ResolvedInner) }` 或 `Option<Box<ResolvedInner>>` | Go 用 `*resolved == nil` 表"继续搜索"、`&resolved{}` 表"明确未解析"、有 `path` 表"已解析"三态。**用判别枚举显式化这三态**（PORTING §3） |
| `NodeResolutionFeatures int32` + iota 位 | `bitflags! NodeResolutionFeatures: i32`（含 `All`/`Node16Default`/`NodeNextDefault`/`BundlerDefault` 组合常量） | flags 用 bitflags（PORTING §3） |
| `extensions int32` + iota 位 | `bitflags! Extensions: i32`（`TypeScript`/`JavaScript`/`Declaration`/`Json` + `ImplementationFiles` 组合）；`.String()`/`.Array()` → `to_string()`/`to_array()` | |
| `ModeAwareCache[T] = map[ModeAwareCacheKey]T` | `type ModeAwareCache<T> = FxHashMap<ModeAwareCacheKey, T>` | key `{Name, Mode}` |
| `caches{ packageJsonInfoCache *InfoCache; moduleResolutionCache; typeRefCache; parsedPatternsForPathsOnce sync.Once }` | `struct Caches { package_json_info_cache: Arc<InfoCache>, module_resolution_cache: ..., type_ref_directive_resolution_cache: ..., parsed_patterns_for_paths: OnceCell<ParsedPatterns> }` | `SyncMap` → `DashMap`；`sync.Once` → `OnceCell`（PORTING §3） |
| `moduleResolutionCache{ cache SyncMap[key,*ResolvedModule] }` | `DashMap<Mfor, Arc<ResolvedModule>>` | 并发缓存（PORTING §6） |
| `Resolver{ caches; host; compilerOptions; ... }` | `struct Resolver { caches: Caches, host: Arc<dyn ResolutionHost>, compiler_options: Arc<CompilerOptions>, typings_location, project_name }` | |
| `resolutionState{ ...; parsedPatternsForPathsOnce sync.Once }` | `struct ResolutionState<'r> { resolver: &'r Resolver, tracer: Option<Tracer>, ... }` | per-request 短生命周期状态；可借用 resolver |
| `tracer{ traces []DiagAndArgs }`（可空） | `struct Tracer { traces: Vec<DiagAndArgs> }`，可空场景用 `Option<Tracer>`；`write` 在 None 时 no-op | Go 用 nil-receiver 容忍；Rust 用 `Option` + 方法在 `&mut self` |
| `resolutionKindSpecificLoader = func(extensions, candidate) *resolved` | 闭包 `impl Fn(Extensions, &str) -> Resolved`（或方法指针枚举） | |
| `PackageId{ Name; SubModuleName; Version; PeerDependencies }` | 同名 struct；`String()`/`PackageName()` → `Display`/`package_name()` | |

### 并发与缓存竞态（必须保持的 bug-fix 语义）

`getPackageJsonInfo` → `packageJsonInfoCache.Set`（内部 `LoadOrStore`）在并发下两个 goroutine 可能都 miss、都去读盘、再都 store；输的一方拿到赢家的 entry。`loadNodeModuleFromDirectoryWorker` 随后用 `ComparePaths(candidate, packageInfo.PackageDirectory)` 守卫——若 candidate（`pkg` vs `pkg/` 拖尾斜杠）与缓存 entry 的目录不一致就会跳过 `main`/`types` 加载导致幻觉 TS2307。修复点是规范化 candidate。Rust 移植必须：① `InfoCache` 用 `entry().or_insert`（原子 LoadOrStore）；② 复刻 candidate 规范化。对应回归测试 `TestResolveModuleNameTrailingSlash` / `...Race` 必须能复现并通过。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/module/resolver.go` | `internal/module/lib.rs` | crate 根。`Resolver`/`resolutionState`/`tracer`/`resolved` + 全部解析算法（~60 个函数） |
| `internal/module/types.go` | `internal/module/types.rs` | `ResolutionHost`、`ModeAwareCacheKey`、`NodeResolutionFeatures`、`PackageId`、`ResolvedModule`、`ResolvedTypeReferenceDirective`、`extensions` |
| `internal/module/cache.go` | `internal/module/cache.rs` | `ModeAwareCache`、`moduleResolutionCache`、`typeRefDirectiveResolutionCache`、`caches`、`newCaches`、`getRedirectConfigName` |
| `internal/module/util.go` | `internal/module/util.rs` | `IsApplicableVersionedTypesKey`、`ParseNodeModuleFromPath`、`ParsePackageName`、`MangleScopedPackageName`/`Unmangle...`、`GetTypesPackageName`/`GetPackageNameFromTypesPackageName`、`ComparePatternKeys`、`GetResolutionDiagnostic`、`TryGetJSExtensionForFile` |

## 依赖白名单（本包新增的 crate）

- `dashmap`（并发缓存）、`bitflags`、`rustc_hash`（FxHashMap）——均在 PORTING §10。
- 正则（`exports` 通配 trailer 匹配若用到）：`regex`（按需，记 `crate-map.md`）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `types.rs`（Go: `types.go`）

- [ ] `pub trait ResolutionHost { fn fs; fn get_current_directory }`　`// Go: types.go:ResolutionHost`
- [ ] `pub struct ModeAwareCacheKey { name: String, mode: ResolutionMode }`　`// Go: types.go:ModeAwareCacheKey`
- [ ] `pub trait ResolvedProjectReference { fn config_name; fn compiler_options }`　`// Go: types.go:ResolvedProjectReference`
- [ ] `bitflags! NodeResolutionFeatures`（Imports/SelfName/Exports/ExportsPatternTrailers/ImportsPatternRoot + None/All/Node16Default/NodeNextDefault/BundlerDefault）　`// Go: types.go:NodeResolutionFeatures`
- [ ] `pub struct PackageId{ name, sub_module_name, version, peer_dependencies }` + `Display`(`pkg@ver+peer`) + `package_name()`　`// Go: types.go:PackageId`
- [ ] `pub struct ResolvedModule{ resolution_diagnostics, resolved_file_name, original_path, extension, resolved_using_ts_extension, package_id, is_external_library_import, alternate_result }` + `is_resolved()`　`// Go: types.go:ResolvedModule`
- [ ] `pub struct ResolvedTypeReferenceDirective{ ... primary, ... }` + `is_resolved()`　`// Go: types.go:ResolvedTypeReferenceDirective`
- [ ] `bitflags! Extensions` + `to_string()` + `to_array()`（按 TS/JS/Declaration/Json 拼 `tspath` 扩展名列表）　`// Go: types.go:extensions`

### `cache.rs`（Go: `cache.go`）

- [ ] `type ModeAwareCache<T> = FxHashMap<ModeAwareCacheKey, T>`　`// Go: cache.go:ModeAwareCache`
- [ ] `struct ModuleResolutionCacheKey{ containing_directory, module_name, resolution_mode, redirect_config_name }` + `ModuleResolutionCache{ DashMap }` + `get`/`set`　`// Go: cache.go:moduleResolutionCache`
- [ ] `struct TypeRefDirectiveResolutionCacheKey{ ..., from_inferred_types_containing_file }` + cache + `get`/`set`　`// Go: cache.go:typeRefDirectiveResolutionCache`
- [ ] `struct Caches{ package_json_info_cache, module_resolution_cache, type_ref_directive_resolution_cache, parsed_patterns_for_paths: OnceCell }`　`// Go: cache.go:caches`
- [ ] `fn new_caches(current_directory, use_case_sensitive_file_names, options) -> Caches`　`// Go: cache.go:newCaches`
- [ ] `fn get_redirect_config_name(redirect: Option<&dyn ResolvedProjectReference>) -> String`　`// Go: cache.go:getRedirectConfigName`

### `util.rs`（Go: `util.go`）

- [ ] `pub const INFERRED_TYPES_CONTAINING_FILE: &str = "__inferred type names__.ts"`　`// Go: util.go`
- [ ] `pub fn is_applicable_versioned_types_key(key) -> bool`（`types@<range>` 且 range.test(tsVersion)）　`// Go: util.go:IsApplicableVersionedTypesKey`
- [ ] `pub fn parse_node_module_from_path(resolved, is_folder) -> String`　`// Go: util.go:ParseNodeModuleFromPath`
- [ ] `pub fn parse_package_name(module_name) -> (String, String)`（处理 `@scope/`）　`// Go: util.go:ParsePackageName`
- [ ] `pub fn mangle_scoped_package_name` / `unmangle_scoped_package_name`（`@a/b`↔`a__b`）　`// Go: util.go:MangleScopedPackageName/UnmangleScopedPackageName`
- [ ] `pub fn get_types_package_name` / `get_package_name_from_types_package_name`（`@types/` 前缀）　`// Go: util.go:GetTypesPackageName/GetPackageNameFromTypesPackageName`
- [ ] `pub fn compare_pattern_keys(a, b) -> i32`（`*` 通配 key 排序：base 长者优先、无 `*` 者更长…）　`// Go: util.go:ComparePatternKeys`
- [ ] `pub fn get_resolution_diagnostic(options, resolved_module, file) -> Option<&'static Message>`（按扩展名 + jsx/allowJs/resolveJsonModule/allowArbitraryExtensions 选诊断）　`// Go: util.go:GetResolutionDiagnostic`
- [ ] `pub fn try_get_js_extension_for_file(file_name, options) -> String`（`.ts`/`.d.ts`→`.js`，`.tsx`→jsx 时 `.jsx` 否则 `.js`，`.mts`→`.mjs` 等）　`// Go: util.go:TryGetJSExtensionForFile`

### `lib.rs`（Go: `resolver.go`，按子区块勾选）

- [ ] `enum Resolved { ContinueSearching, Unresolved, Found{ path, extension, package_id, original_path, resolved_using_ts_extension } }` + `is_resolved()`/`should_continue_searching()`　`// Go: resolver.go:resolved`
- [ ] `struct Tracer{ traces: Vec<DiagAndArgs> }` + `write`/`get_traces`；`struct DiagAndArgs{ message, args }`　`// Go: resolver.go:tracer`
- [ ] `struct ResolutionState{ request 字段 + state 字段 }` + `new_resolution_state`（按 ModuleResolutionKind 设 features/esmMode/conditions/extensions）　`// Go: resolver.go:resolutionState/newResolutionState`
- [ ] `pub fn get_compiler_options_with_redirect(options, redirect) -> &CompilerOptions`　`// Go: resolver.go:GetCompilerOptionsWithRedirect`
- [ ] `pub struct Resolver{ caches, host, compiler_options, typings_location, project_name }`　`// Go: resolver.go:Resolver`
- [ ] `pub fn new_resolver` / `new_resolver_with_options`（`ResolverOptions{ package_json_cache }`）　`// Go: resolver.go:NewResolver/NewResolverWithOptions`
- [ ] **公开入口**：
  - [ ] `pub fn resolve_module_name(&self, module_name, containing_file, resolution_mode, redirect) -> (ResolvedModule, Vec<DiagAndArgs>)`　`// Go: resolver.go:ResolveModuleName`
  - [ ] `pub fn resolve_type_reference_directive(...) -> (ResolvedTypeReferenceDirective, Vec<DiagAndArgs>)`　`// Go: resolver.go:ResolveTypeReferenceDirective`
  - [ ] `pub fn resolve_package_directory(...)`　`// Go: resolver.go:ResolvePackageDirectory`
  - [ ] `pub fn get_package_scope_for_path(&self, dir) -> Option<InfoCacheEntry>`　`// Go: resolver.go:GetPackageScopeForPath`
  - [ ] `pub fn get_entrypoints_from_package_json_info(...)` + `ResolvedEntrypoint`/`SymlinkOrRealpath`　`// Go: resolver.go:GetEntrypointsFromPackageJsonInfo`
- [ ] **node-like 解析链**（`resolveNodeLike`/`Worker`、`loadModuleFromSelfNameReference`、`loadModuleFromImports`、`loadModuleFromExports`、`loadModuleFromExportsOrImports`、`loadModuleFromTargetExportOrImport`、`tryLoadInputFileForPath`、`getOutputDirectoriesForBaseDirectory`）　`// Go: resolver.go:resolveNodeLike...`
- [ ] **node_modules 上溯**（`loadModuleFromNearestNodeModulesDirectory[Worker]`、`loadModuleFromImmediateNodeModulesDirectory`、`loadModuleFromSpecificNodeModulesDirectory`）　`// Go: resolver.go:loadModuleFromNearestNodeModulesDirectory...`
- [ ] **文件加载 / 扩展名**（`nodeLoadModuleByRelativeName`、`loadModuleFromFile[NoImplicitExtensions]`、`tryAddingExtensions`、`tryExtension`、`tryFile[Lookup]`、`extensionIsOk`）　`// Go: resolver.go:loadModuleFromFile...`
- [ ] **package.json 目录加载**（`loadNodeModuleFromDirectory[Worker]`、`loadFileNameFromPackageJSONField`、`getPackageFile`、`getPackageJsonInfo`、`getPackageId`、`readPackageJsonPeerDependencies`、`validatePackageJSONField`、`getPackageJSONPathField`）—— **注意 candidate 规范化竞态修复**　`// Go: resolver.go:loadNodeModuleFromDirectoryWorker/getPackageJsonInfo`
- [ ] **paths / rootDirs**（`tryLoadModuleUsingOptionalResolutionSettings`、`tryLoadModuleUsingPathsIfEligible`、`tryLoadModuleUsingPaths`、`tryLoadModuleUsingRootDirs`、`getParsedPatternsForPaths`、`TryParsePatterns`、`MatchPatternOrExact`、`matchesPatternWithTrailer`、`replaceFirstStar` 等）　`// Go: resolver.go:tryLoadModuleUsingPaths...`
- [ ] **typeRoots / type ref**（`resolveTypeReferenceDirective`、`getCandidateFromTypeRoot`、`resolveFromTypeRoot`、`mangleScopedPackageName`）　`// Go: resolver.go:resolveTypeReferenceDirective`
- [ ] **typings location 回退**（`tryResolveFromTypingsLocation`）、**config 解析**（`resolveConfig`、`ResolveConfig`）　`// Go: resolver.go:tryResolveFromTypingsLocation/ResolveConfig`
- [ ] **结果构造 + symlink**（`createResolvedModule[HandlingSymlink]`、`createResolvedTypeReferenceDirective`、`getOriginalAndResolvedFileName`、`realPath`）　`// Go: resolver.go:createResolvedModule...`
- [ ] **条件 / 特性**（`conditionMatches`、`GetConditions`、`getNodeResolutionFeatures`、`getTraceFunc`）　`// Go: resolver.go:GetConditions/getNodeResolutionFeatures`
- [ ] **entrypoints 导出枚举**（`loadEntrypointsFromExportMap`、`getMatchedStarForPatternEntrypoint`、`createResolvedEntrypointHandlingSymlink`）　`// Go: resolver.go:loadEntrypointsFromExportMap`
- [ ] `pub fn get_automatic_type_directive_names(options, host) -> Vec<String>`　`// Go: resolver.go:GetAutomaticTypeDirectiveNames`
- [ ] 工具：`normalizePathForCJSResolution`、`moveToNextDirectorySeparatorIfAvailable`　`// Go: resolver.go`

### Cargo / crate 接线

- [ ] `internal/module/Cargo.toml`（`name = "tsgo_module"` + path deps）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明 `mod types; mod cache; mod util;` + re-export 公开 API

## TDD 推进顺序（tracer bullet → 增量）

1. `util.rs` 全是纯字符串函数（`ParsePackageName`/`MangleScopedPackageName`/`ComparePatternKeys`/`TryGetJSExtensionForFile`…）→ 可独立先写 + 补行为级单测（见 tests.md）。
2. `types.rs` + `cache.rs` 数据结构 + bitflags 位值快照。
3. `Resolved` 三态枚举 + `tracer`。
4. 最小 `resolve_module_name` 路径：relative + 加扩展名（`tryAddingExtensions`/`tryFile`）；用 `vfstest::from_map` 起一个内存 vfs → 通过 `TestResolveModuleNameTrailingSlash`。
5. node_modules 上溯 + `package.json` `main`/`types` + candidate 规范化 → 通过 `...Race`（并发）。
6. 逐步补 `exports`/`imports`/`paths`/`rootDirs`/`typeRoots`/`typesVersions`（这些主要靠 P10 conformance 验证）。

## 与 Go 的已知偏离（divergence）

- `*resolved` 三态（nil/空/有值）→ `enum Resolved`（显式三分支）。
- `tracer` nil-receiver 容忍 → `Option<Tracer>` + `&mut self`。
- `SyncMap` → `DashMap`，`sync.Once` → `OnceCell`；并发竞态语义（LoadOrStore）保持。
- vfs I/O 通过 `&dyn Vfs`（P1）；测试用 `vfstest::from_map`。
- 大量 `func(...) *resolved` 链式"继续搜索 / 命中"控制流：用 `?`-风格 early-return + `Option`/`Resolved` 匹配表达，结构 1:1。

## 转交 / 推迟（DEFER）

- `exports`/`imports`/`paths`/`typeRoots` 等的**完整正确性**主要靠 P10 conformance（`tests/cases/conformance/moduleResolution/*`）端到端兜底；本包单测仅 2 个回归 + util 行为级。
- 依赖 `tsgo_vfs`（含 `vfsmatch`）、`tsgo_tspath`、`tsgo_semver`、`tsgo_packagejson`——除 `packagejson`（同 phase 前序）外均 P1 已就绪。
