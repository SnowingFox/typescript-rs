# outputpaths: 实现方案（impl.md）

**crate**：`tsgo_outputpaths`　**目标**：根据 `CompilerOptions` + 源文件名计算每个文件的输出路径（`.js` / `.js.map` / `.d.ts` / `.d.ts.map` / `.tsbuildinfo`），并计算 common source directory。
**依赖（crate）**：`tsgo_ast` `tsgo_core` `tsgo_tspath`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/outputpaths/`（2 个非测试文件，约 300 行）

## 这个包是什么（业务说明）

emit 阶段在真正打印代码之前，必须先决定"产物写到哪里"。`outputpaths` 就是这层纯路径运算：给定一个 `*ast.SourceFile` 和 `*core.CompilerOptions`，算出它对应的 `.js`、source map、`.d.ts`、声明 map 的绝对路径，以及（增量编译时）`.tsbuildinfo` 的位置。

它还负责 **common source directory** 的推导：当用户没有显式 `rootDir` 时，TypeScript 用"所有输入文件的最长公共目录前缀"作为 `outDir` 重映射的基准，使得 `src/a/b.ts → outDir/a/b.js` 这种相对结构得以保留。

本包是纯函数 + 少量小 struct，无 I/O（文件是否存在的判断全部下放给上层 host 接口），是 Phase 5 里依赖最少、最适合做 tracer bullet 的包。上游主要被 `compiler`（P6）和 `transformers/declarations` 调用。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3。本包特有点：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `OutputPathsHost interface` | `pub trait OutputPathsHost` | 3 方法：`common_source_directory`/`get_current_directory`/`use_case_sensitive_file_names`；上层（compiler）实现 |
| `OutputPaths struct`（4 私有 string 字段 + getter） | `pub struct OutputPaths { js_file_path: String, source_map_file_path: String, declaration_file_path: String, declaration_map_path: String }` | 字段私有、配 `pub fn` getter，1:1 对齐 Go 的访问器；空字符串 = "无此产物" |
| `func(...) bool` 回调（`ForEachEmittedFile` 的 action、`GetCommonSourceDirectory` 的 `checkSourceFilesBelongToPath`） | `impl FnMut(...) -> bool` / `Option<impl Fn(...)>` | Go 用裸函数值，Rust 用闭包 trait bound |
| `files func() []string`（惰性取文件名） | `impl Fn() -> Vec<String>` | 保留惰性，避免在不需要时构建文件名列表 |
| `outputDir *string`（Go 用指针区分"未设置 vs 空串"） | `Option<&str>` | `GetDeclarationEmitOutputFilePath` 里区分 declarationDir/outDir/无 |
| `core.TSTrue` / `IsTrue()` 三态布尔 | `core::Tristate`（沿用 tsgo_core 决策） | `EmitDeclarationOnly != core.TSTrue` 等条件要 1:1 |

> 本包不持有 AST/arena，只读 `sourceFile.FileName()`，所以无所有权图复杂度。`*ast.SourceFile` 入参用 `&SourceFile`（来自 `tsgo_ast` arena 的只读借用或 `NodeId` + arena，按 P2 ast 落地方式对齐）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/outputpaths/outputpaths.go` | `internal/outputpaths/lib.rs` | crate 根：`OutputPathsHost`/`OutputPaths` + 全部输出路径函数（basename == 目录名 → `lib.rs`） |
| `internal/outputpaths/commonsourcedirectory.go` | `internal/outputpaths/commonsourcedirectory.rs` | common source directory 推导；`lib.rs` 里 `mod commonsourcedirectory;` |

## 依赖白名单（本包新增的 crate）

无 PORTING §10 之外的新增 crate。仅用 std + `tsgo_tspath` 路径工具。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/outputpaths/outputpaths.go`）

- [ ] `pub trait OutputPathsHost` — `common_source_directory()/get_current_directory()/use_case_sensitive_file_names()`　`// Go: outputpaths.go:OutputPathsHost`
- [ ] `pub struct OutputPaths` + 4 getter `pub fn js_file_path/source_map_file_path/declaration_file_path/declaration_map_path(&self) -> &str`　`// Go: outputpaths.go:OutputPaths`
- [ ] `pub fn get_output_paths_for(source_file, options, host, force_dts_emit) -> OutputPaths` — 组装四类路径；处理 json 文件同位置跳过、`EmitDeclarationOnly`、`forceDtsEmit`　`// Go: outputpaths.go:GetOutputPathsFor`
- [ ] `pub fn for_each_emitted_file(host, options, action, source_files, force_dts_emit) -> bool` — 遍历直到 action 返回 true　`// Go: outputpaths.go:ForEachEmittedFile`
- [ ] `pub fn get_output_js_file_name(input, options, host) -> String` — `EmitDeclarationOnly` 返回空；json 同位置返回空　`// Go: outputpaths.go:GetOutputJSFileName`
- [ ] `pub fn get_output_js_file_name_worker(input, options, host) -> String`　`// Go: outputpaths.go:GetOutputJSFileNameWorker`
- [ ] `pub fn get_output_declaration_file_name_worker(input, options, host) -> String` — declarationDir/outDir 选择 + `GetDeclarationEmitExtensionForPath`　`// Go: outputpaths.go:GetOutputDeclarationFileNameWorker`
- [ ] `pub fn get_output_extension(file_name, jsx) -> &'static str` — json/jsx/mjs/cjs/js 分派（match）　`// Go: outputpaths.go:GetOutputExtension`
- [ ] `pub fn get_declaration_emit_output_file_path(file, options, host) -> String`　`// Go: outputpaths.go:GetDeclarationEmitOutputFilePath`
- [ ] `pub fn get_source_file_path_in_new_dir(file_name, new_dir, cur_dir, common_src_dir, case_sensitive) -> String` — 用 `ContainsPath` 判前缀　`// Go: outputpaths.go:GetSourceFilePathInNewDir`
- [ ] `pub fn get_source_file_path_in_new_dir_worker(...) -> String` — 用 `HasPrefix(canonFile, commonDir)` 判前缀（注意与上一函数判定方式不同，见偏离）　`// Go: outputpaths.go:GetSourceFilePathInNewDirWorker`
- [ ] `fn get_output_path_without_changing_extension(input, output_dir, host) -> String`（私有）　`// Go: outputpaths.go:getOutputPathWithoutChangingExtension`
- [ ] `fn get_own_emit_output_file_path(file_name, options, host, extension) -> String`（私有）　`// Go: outputpaths.go:getOwnEmitOutputFilePath`
- [ ] `pub fn get_source_map_file_path(js_file_path, options) -> String` — `SourceMap && !InlineSourceMap` 才返回 `path + ".map"`　`// Go: outputpaths.go:GetSourceMapFilePath`
- [ ] `pub fn get_build_info_file_name(options, opts: ComparePathsOptions) -> String` — incremental/build 判定 + TsBuildInfoFile/ConfigFilePath/outDir/rootDir 组合　`// Go: outputpaths.go:GetBuildInfoFileName`

### `commonsourcedirectory.rs`（Go: `internal/outputpaths/commonsourcedirectory.go`）

- [ ] `fn compute_common_source_directory_of_filenames(file_names, cur_dir, case_sensitive) -> String`（私有）— 逐文件取规范化路径分量、求公共前缀；全 `.d.ts` 时回退 `currentDirectory`　`// Go: commonsourcedirectory.go:computeCommonSourceDirectoryOfFilenames`
- [ ] `pub fn get_computed_common_source_directory(emitted_files, cur_dir, case_sensitive) -> String` — 上者 + `EnsureTrailingDirectorySeparator`　`// Go: commonsourcedirectory.go:GetComputedCommonSourceDirectory`
- [ ] `pub fn get_common_source_directory(options, files: impl Fn()->Vec<String>, cur_dir, case_sensitive, check_belong: Option<...>) -> String` — rootDir / configFilePath 目录 / 计算 三分支 + 尾分隔符　`// Go: commonsourcedirectory.go:GetCommonSourceDirectory`

### Cargo / crate 接线

- [ ] `internal/outputpaths/Cargo.toml`（`name = "tsgo_outputpaths"` + path deps：`tsgo_ast` `tsgo_core` `tsgo_tspath`）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] `lib.rs` 声明 `mod commonsourcedirectory;` + `pub use`

## TDD 推进顺序（tracer bullet → 增量）

1. `get_output_extension`（纯 match，无 host，最易；先建 crate 骨架 + 该函数行为级测试）。
2. `compute_common_source_directory_of_filenames` + `get_computed_common_source_directory`（核心算法，可用 Go 实测目录前缀值断言）。
3. `OutputPaths` struct + getter + `get_source_map_file_path`（最小路径组合）。
4. `get_output_js_file_name_worker` / `get_own_emit_output_file_path`（依赖 tspath 的 `ResolvePath`/`ChangeExtension`/`GetRelativePathFromDirectory`）。
5. `get_output_paths_for`（顶层组装，串起以上全部）。
6. `get_build_info_file_name` / `for_each_emitted_file`（收尾）。

## 与 Go 的已知偏离（divergence）

- **两套前缀判定并存**：`GetSourceFilePathInNewDir` 用 `tspath.ContainsPath`（带 case 选项的分量比较），而 `GetSourceFilePathInNewDirWorker` 用 `strings.HasPrefix(canonFile, commonDir)`（裸字符串前缀）。这是 Go 上游历史遗留的两个实现，**必须 1:1 保留两者各自的判定方式**，不要统一，否则边界路径会偏。
- `outputDir *string` 的"nil vs 空串"语义 → `Option<&str>`，注意 Go 里 `len(options.OutDir) > 0` 的判断要对齐（空串当作未设置）。

## 转交 / 推迟（DEFER）

- 本包 Go 侧 0 直接单测，行为正确性最终由 **P10 conformance/`tsc --outDir/--declaration` baseline** 兜底；本轮补行为级 Rust 测试（见 tests.md）。
- `*ast.SourceFile` 的具体借用形态依赖 P2 `tsgo_ast` 的 arena 落地；接口签名按"只读 source file + FileName()"对齐，落地时回填。
