# outputpaths: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例（**Go 侧无直接单测**）。

## 0 直接单测的情况

- Go 侧 `internal/outputpaths/` **无 `*_test.go`**；该包行为由 **P10 conformance parity** 兜底（`tsc` 的 `--outDir` / `--rootDir` / `--declaration` / `--declarationDir` / `--sourceMap` / `--inlineSourceMap` / `--tsBuildInfoFile` baseline 对拍）。
- 本轮补充少量**行为级 Rust 测试**（基于公开接口，expected 取自 TS 已知路径规则 / Go 实测；执行期建一个最小 `FakeHost` 实现 `OutputPathsHost`）。

### 补充行为级测试（建议 `internal/outputpaths/lib.rs` 内 `#[cfg(test)] mod tests`）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `get_output_extension_js` | 普通 ts → `.js` | `("/a/b.ts", JsxEmitNone)` → `".js"` | `GetOutputExtension` default 分支 | |
| `get_output_extension_json` | json 保持 `.json` | `("/a/b.json", _)` → `".json"` | json 分支 | |
| `get_output_extension_jsx_preserve` | preserve 下 tsx→jsx | `("/a/b.tsx", JsxEmitPreserve)` → `".jsx"` | jsx 分支 | |
| `get_output_extension_mts` | mts→mjs | `("/a/b.mts", JsxEmitNone)` → `".mjs"` | mts/mjs 分支 | |
| `get_output_extension_cts` | cts→cjs | `("/a/b.cts", JsxEmitNone)` → `".cjs"` | cts/cjs 分支 | |
| `common_source_dir_single` | 单文件取其目录 | files=`["/src/a.ts"]` → `"/src/"`（含尾分隔符） | `GetComputedCommonSourceDirectory` | |
| `common_source_dir_multi` | 多文件最长公共前缀 | files=`["/src/a/x.ts","/src/a/y.ts","/src/b/z.ts"]` → `"/src/"` | `computeCommonSourceDirectoryOfFilenames` | |
| `common_source_dir_no_common` | 无公共前缀 → 空 | files=`["/a/x.ts","/b/y.ts"]`（i==0 失配）→ `""` | `i == 0` 返回 `""` | |
| `common_source_dir_all_dts` | 全 `.d.ts` 回退 cwd | files=`["/x/a.d.ts"]`, cwd=`/cwd` → `"/cwd"`（计算函数返回 cwd） | "Can happen when all input files are .d.ts" 分支 | |
| `source_map_path_enabled` | sourceMap on / inline off | `("/out/a.js", {SourceMap:true})` → `"/out/a.js.map"` | `GetSourceMapFilePath` | |
| `source_map_path_inline_off` | inlineSourceMap 抑制独立 map | `("/out/a.js", {SourceMap:true,InlineSourceMap:true})` → `""` | `!InlineSourceMap` 条件 | |
| `js_path_with_outdir` | outDir 重映射保留相对结构 | host.commonSrc=`/src/`, `("/src/a/b.ts", {OutDir:"/out"})` → `/out/a/b.js` | `getOwnEmitOutputFilePath`+`get_output_paths_for` | |
| `decl_path_with_declaration_dir` | declarationDir 优先于 outDir | `{OutDir:"/out",DeclarationDir:"/types"}` → `/types/a/b.d.ts` | `GetDeclarationEmitOutputFilePath` | |
| `decl_map_path` | 声明 map = decl + ".map" | declarationMaps on → `<decl>.d.ts.map` | `GetOutputPathsFor` declarationMapPath | |
| `emit_declaration_only_no_js` | EmitDeclarationOnly 抑制 js/map | `{EmitDeclarationOnly:true}` → `js_file_path()==""` | `GetOutputPathsFor` 首个 if | |
| `json_emitted_same_location_skip` | json 输出与输入同位置则不写 js | 无 outDir 的 `/a.json` → `js_file_path()==""` | `isJsonEmittedToSameLocation` | |
| `build_info_none_when_not_incremental` | 非 incremental/build → 空 | `{}` → `""` | `GetBuildInfoFileName` 首个 if | |
| `build_info_explicit_file` | 显式 tsBuildInfoFile 直接用 | `{Incremental:true,TsBuildInfoFile:"/x.tsbuildinfo"}` → `"/x.tsbuildinfo"` | `TsBuildInfoFile != ""` | |
| `build_info_from_config_outdir_rootdir` | outDir+rootDir 重映射 config 名 | `{Incremental, ConfigFilePath:"/p/tsconfig.json", OutDir:"/out", RootDir:"/p"}` → `/out/tsconfig.tsbuildinfo` | outDir+rootDir 分支 | |

## 与 impl.md 的对齐核对

- [ ] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/outputpaths/outputpaths.go:<Func>`，因 Go 侧无 `*_test.go`）

- [ ] impl.md 中每个 `pub fn` 都有上表至少一条行为级用例承载（或标注由 P10 兜底）
- [ ] expected 值取自 TS 已知路径规则 / Go 实测（非 Rust 推断）
- [ ] 每条用例可由最小 `FakeHost` 驱动（无真实文件 I/O）
- [ ] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `--outDir`/`--declaration`/`--sourceMap` 全量路径 baseline | 需真实 tsc 输出对拍 | P10 |
| common source directory 在大型多包工程下的边界 | 需 program 级 fixtures | P10 |
