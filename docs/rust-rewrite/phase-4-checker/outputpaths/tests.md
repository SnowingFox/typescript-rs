# outputpaths: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例（**Go 侧无直接单测**）。

## 0 直接单测的情况

- Go 侧 `internal/outputpaths/` **无 `*_test.go`**；该包行为由 **P10 conformance parity** 兜底（`tsc` 的 `--outDir` / `--rootDir` / `--declaration` / `--declarationDir` / `--sourceMap` / `--inlineSourceMap` / `--tsBuildInfoFile` baseline 对拍）。
- 本轮补充少量**行为级 Rust 测试**（基于公开接口，expected 取自 TS 已知路径规则 / Go 实测；执行期建一个最小 `FakeHost` 实现 `OutputPathsHost`）。

### 补充行为级测试（独立兄弟文件 `lib_test.rs` / `commonsourcedirectory_test.rs`，`use super::*;` 挂载）

> 落地说明：按 PORTING §2/§8，单测放兄弟文件而非内联 `#[cfg(test)] mod tests`。
> `lib.rs` ↔ `lib_test.rs`，`commonsourcedirectory.rs` ↔ `commonsourcedirectory_test.rs`。
> 私有项（`compute_common_source_directory_of_filenames`）经 `use super::*` 直接断言。
> 标注「（修正自规划值）」的两行：规划时凭直觉填的 expected 与 Go 实算不符，已按上游 1:1 行为更正（ground truth 取自已 GREEN 的 `tsgo_tspath` 逐函数 trace）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `get_output_extension_js` | 普通 ts → `.js` | `("/a/b.ts", JsxEmit::None)` → `".js"` | `GetOutputExtension` default 分支 | ✓ |
| `get_output_extension_json` | json 保持 `.json` | `("/a/b.json", _)` → `".json"` | json 分支 | ✓ |
| `get_output_extension_jsx_preserve` | preserve 下 tsx→jsx | `("/a/b.tsx", JsxEmit::Preserve)` → `".jsx"` | jsx 分支 | ✓ |
| `get_output_extension_mts` | mts→mjs | `("/a/b.mts", JsxEmit::None)` → `".mjs"` | mts/mjs 分支 | ✓ |
| `get_output_extension_cts` | cts→cjs | `("/a/b.cts", JsxEmit::None)` → `".cjs"` | cts/cjs 分支 | ✓ |
| `common_source_dir_single` | 单文件取其目录 | files=`["/src/a.ts"]` → `"/src/"`（含尾分隔符） | `GetComputedCommonSourceDirectory` | ✓ |
| `common_source_dir_multi` | 多文件最长公共前缀 | files=`["/src/a/x.ts","/src/a/y.ts","/src/b/z.ts"]` → `"/src/"` | `computeCommonSourceDirectoryOfFilenames` | ✓ |
| `common_source_dir_narrows_to_root` | 共享根但二级目录不同 → 收敛到根 | files=`["/a/x.ts","/b/y.ts"]` → `"/"`（POSIX 同根 `/`，**非** `""`；修正自规划值） | `computeCommonSourceDirectoryOfFilenames` 逐分量收窄 | ✓ |
| `common_source_dir_distinct_roots` | 首个分量失配（`i==0`）→ 空 | files=`["c:/a/x.ts","d:/b/y.ts"]`, cwd=`c:/` → `""`（驱动器根不同才触发 `i==0`；修正自规划值） | `i == 0` 返回 `""` | ✓ |
| `common_source_dir_all_dts_falls_back_to_cwd` | 全 `.d.ts`（过滤后入参为空）回退 cwd | files=`[]`, cwd=`/cwd` → `"/cwd"`（私有计算函数返回 cwd；**空表**才触发该分支；修正自规划值） | "Can happen when all input files are .d.ts" 分支 | ✓ |
| `source_map_path_enabled` | sourceMap on / inline off | `("/out/a.js", {source_map:True})` → `"/out/a.js.map"` | `GetSourceMapFilePath` | ✓ |
| `source_map_path_inline_off` | inlineSourceMap 抑制独立 map | `("/out/a.js", {source_map:True,inline_source_map:True})` → `""` | `!InlineSourceMap` 条件 | ✓ |
| `source_file_path_in_new_dir_uses_contains_path` | **偏离锚①**：`ContainsPath` 分量判定 | `("/src2/a.ts","/out","/","/src",true)` → `"/src2/a.ts"`（不收录，路径原样） | `GetSourceFilePathInNewDir`（`ContainsPath`） | ✓ |
| `source_file_path_in_new_dir_worker_uses_has_prefix` | **偏离锚②**：`HasPrefix` 裸串判定（同输入异结果） | `("/src2/a.ts","/out","/","/src",true)` → `"/out/2/a.ts"` | `GetSourceFilePathInNewDirWorker`（`strings.HasPrefix`） | ✓ |
| `source_file_path_in_new_dir_inside_common_dir` | 文件在 common dir 内 → 收录重映射 | `("/src/a/b.ts","/out","/","/src/",true)` → `"/out/a/b.ts"` | `GetSourceFilePathInNewDir` | ✓ |
| `js_path_with_outdir` | outDir 重映射保留相对结构 | host.commonSrc=`/src/`, `("/src/a/b.ts", {out_dir:"/out"})` → `/out/a/b.js` | `getOwnEmitOutputFilePath`+`GetOutputPathsFor` | ✓ |
| `decl_path_with_declaration_dir` | declarationDir 优先于 outDir | `{out_dir:"/out",declaration_dir:"/types"}` → `/types/a/b.d.ts` | `GetDeclarationEmitOutputFilePath` | ✓ |
| `decl_map_path` | 声明 map = decl + ".map" | `{declaration:True,declaration_map:True}`, `/src/a/b.ts` → decl `/src/a/b.d.ts`, map `/src/a/b.d.ts.map` | `GetOutputPathsFor` declarationMapPath | ✓ |
| `emit_declaration_only_no_js` | EmitDeclarationOnly 抑制 js/map | `{emit_declaration_only:True}` → `js_file_path()==""` | `GetOutputPathsFor` 首个 if | ✓ |
| `json_emitted_same_location_skip` | json 输出与输入同位置则不写 js | 无 outDir 的 `/a.json` → `js_file_path()==""` | `isJsonEmittedToSameLocation` | ✓ |
| `get_output_js_file_name_outdir` | outDir 下 worker 重映射 | `("/src/a.ts", {out_dir:"/out"})` → `"/out/a.js"` | `GetOutputJSFileName` | ✓ |
| `get_output_js_file_name_emit_declaration_only` | EmitDeclarationOnly → 空 | `{emit_declaration_only:True}` → `""` | `GetOutputJSFileName` 首个 if | ✓ |
| `get_output_js_file_name_json_same_location` | json 输出=输入 → 空 | 无 outDir 的 `/a.json` → `""` | `GetOutputJSFileName` 末尾 json 守卫 | ✓ |
| `get_output_js_file_name_worker_outdir` | worker 不含 json 守卫 | `("/src/a/b.ts", {out_dir:"/out"})` → `"/out/a/b.js"` | `GetOutputJSFileNameWorker` | ✓ |
| `get_output_declaration_file_name_worker_outdir` | 声明 worker（扩展取自 input） | `("/src/a/b.ts", {out_dir:"/out"})` → `"/out/a/b.d.ts"` | `GetOutputDeclarationFileNameWorker` | ✓ |
| `build_info_none_when_not_incremental` | 非 incremental/build → 空 | `{}` → `""` | `GetBuildInfoFileName` 首个 if | ✓ |
| `build_info_explicit_file` | 显式 tsBuildInfoFile 直接用 | `{incremental:True,ts_build_info_file:"/x.tsbuildinfo"}` → `"/x.tsbuildinfo"` | `TsBuildInfoFile != ""` | ✓ |
| `build_info_from_config_outdir_rootdir` | outDir+rootDir 重映射 config 名 | `{incremental, config_file_path:"/p/tsconfig.json", out_dir:"/out", root_dir:"/p"}` → `/out/tsconfig.tsbuildinfo` | outDir+rootDir 分支 | ✓ |
| `build_info_from_config_outdir_no_rootdir` | outDir 无 rootDir → 取 base 名 | `{incremental, config_file_path:"/p/sub/tsconfig.json", out_dir:"/out"}` → `/out/tsconfig.tsbuildinfo` | outDir 无 rootDir 分支（`CombinePaths`+`GetBaseFileName`） | ✓ |
| `build_info_from_config_no_outdir` | 无 outDir → config 同目录 | `{incremental, config_file_path:"/p/tsconfig.json"}` → `/p/tsconfig.tsbuildinfo` | 无 outDir 分支 | ✓ |
| `build_info_incremental_no_config` | incremental 但无 config → 空 | `{incremental:True}` → `""` | `ConfigFilePath == ""` 守卫 | ✓ |
| `common_source_directory_root_dir` | rootDir 分支 | `{root_dir:"/r"}` → `"/r/"` | `GetCommonSourceDirectory` rootDir | ✓ |
| `common_source_directory_config_file` | configFilePath 分支取其目录 | `{config_file_path:"/p/tsconfig.json"}` → `"/p/"` | `GetCommonSourceDirectory` config | ✓ |
| `common_source_directory_computed` | 计算分支 | files=`["/src/a.ts"]` → `"/src/"` | `GetCommonSourceDirectory` compute | ✓ |
| `common_source_directory_check_callback_invoked` | check 回调被以选定 path 调用 | `{root_dir:"/r"}`, 回调记录 path → 收到 `"/r"`，结果 `"/r/"` | `checkSourceFilesBelongToPath` 调用点 | ✓ |
| `for_each_emitted_file_visits_all` | 遍历全部、action 全 false → false | 2 文件 → 收集到 `["/out/a.js","/out/b.js"]`，返回 `false` | `ForEachEmittedFile` 循环 | ✓ |
| `for_each_emitted_file_short_circuits` | action 返回 true 即短路 | action 首次返回 true → 仅调用 1 次，返回 `true` | `ForEachEmittedFile` 提前 return | ✓ |

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（每个 `#[test]` 上方一行 `// Go: internal/outputpaths/<file>.go:<Func>`，因 Go 侧无 `*_test.go`，锚指向实现源）
- [x] impl.md 中每个 `pub fn` 都有上表至少一条行为级用例承载（含两套前缀判定各自的偏离锚；`SourceFileLike` 经 `FakeSourceFile` 驱动）
- [x] expected 值取自 TS 已知路径规则 / Go 实测（逐函数 trace 已 GREEN 的 `tsgo_tspath`，非 Rust 推断）；3 处规划值已更正并标注
- [x] 每条用例可由最小 `FakeHost`/`FakeSourceFile` 驱动（无真实文件 I/O）
- [x] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `--outDir`/`--declaration`/`--sourceMap` 全量路径 baseline | 需真实 tsc 输出对拍 | P10 |
| common source directory 在大型多包工程下的边界 | 需 program 级 fixtures | P10 |
