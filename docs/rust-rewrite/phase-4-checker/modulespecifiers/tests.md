# modulespecifiers: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 个测试文件 / 6 个 `func Test*` / 共约 17 个表驱动子用例（含 1 层嵌套）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/modulespecifiers/specifiers_test.go` | `internal/modulespecifiers/lib.rs` + `util.rs`（`#[cfg(test)] mod tests`，用 `MockModuleSpecifierGenerationHost`） | 6 |

> 测试是 `package modulespecifiers`（内部测试包，能调私有 `containsIgnoredPath`/`tryGetModuleNameFromExportsOrImports`）→ Rust 放 `#[cfg(test)] mod tests`（同 crate 可见私有项）。

## `specifiers_test.go`

### `TestGetEachFileNameOfModule`（表驱动，4 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `each_file_basic_path` | 普通文件路径返回自身 | importing `/project/src/main.ts`, imported `/project/lib/utils.ts`, prefer=false → count=1, paths=`["/project/lib/utils.ts"]` | `TestGetEachFileNameOfModule/basic file path` | ✓ |
| `each_file_symlink_pref_false` | preferSymlinks=false | 同上 imported, prefer=false → count=1 | `.../symlink preference false` | ✓ |
| `each_file_symlink_pref_true` | preferSymlinks=true | 同上 imported, prefer=true → count=1 | `.../symlink preference true` | ✓ |
| `each_file_ignored_no_alternatives` | 全 ignored 时至少返回 1 | imported `/project/node_modules/.pnpm/file.ts`, prefer=false → count=1 | `.../ignored path with no alternatives` | ✓ |

> 所有用例额外断言：结果里没有空 `FileName`（Go 末尾循环）。host 用 `MockModuleSpecifierGenerationHost{ current_dir:"/project", use_case_sensitive:true, symlink_cache: new_known_symlink("/project",true) }`。

### `TestGetEachFileNameOfModuleWithSymlinks`（单块）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `each_file_with_symlink_dir` | 命中目录级 symlink 路径 | symlink cache: `/project/symlink/`→real `/real/path/`；`get_each_file_name_of_module("/project/src/main.ts","/real/path/file.ts",host,true)` → 结果含 `/project/symlink/file.ts` | `specifiers_test.go:TestGetEachFileNameOfModuleWithSymlinks` | ✓ |

### `TestContainsNodeModules`（表驱动，4 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `contains_nm_true` | 含 node_modules | `/project/node_modules/lodash/index.js` → true | `.../contains node_modules` | ✓ |
| `contains_nm_false` | 不含 | `/project/src/utils.ts` → false | `.../does not contain node_modules` | ✓ |
| `contains_nm_middle` | 中段 node_modules | `/project/packages/node_modules/pkg/file.js` → true | `.../node_modules in middle` | ✓ |
| `contains_nm_empty` | 空串 | `""` → false | `.../empty path` | ✓ |

### `TestContainsIgnoredPath`（表驱动，2 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `ignored_pnpm` | `.pnpm` 路径被忽略 | `/project/node_modules/.pnpm/file.ts` → true | `.../ignored path` | ✓ |
| `ignored_normal_false` | 普通路径不忽略 | `/project/src/file.ts` → false | `.../not ignored path` | ✓ |

### `TestTryGetRealFileNameForNonJSDeclarationFileName`（表驱动，3 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `real_name_json_decl` | `.d.json.ts` → `.json` | `/project/foo.d.json.ts` → `/project/foo.json` | `.../json declaration file` | ✓ |
| `real_name_multidot_decl` | 多点源扩展声明 | `/project/foo.module.d.css.ts` → `/project/foo.module.css` | `.../multi-dot source extension declaration file` | ✓ |
| `real_name_plain_dts_empty` | 纯 `.d.ts` 返回空 | `/project/foo.d.ts` → `""` | `.../plain dts file ignored` | ✓ |

### `TestTryGetModuleNameFromExportsOrImports`（嵌套 `with exports pattern`，表驱动 2 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `exports_pattern_match` | 通配 `*` 命中 | key `./src/things/*`, target string `./src/things/*/index.js`, targetFilePath `/pkg/src/things/thing1/index.ts`, MatchingMode=Pattern → `"./src/things/thing1"` | `.../with exports pattern/match` | ✓ |
| `exports_pattern_mismatch` | 前后缀匹配但中间不符 → 空 | targetFilePath `/pkg/src/things/index.ts` → `""` | `.../with exports pattern/mismatch with matching leading and trailing strings` | ✓ |

> 调用形参（Go 字面量）：`tryGetModuleNameFromExportsOrImports(&CompilerOptions{}, mockHost{}, targetFilePath, "/pkg", "./src/things/*", ExportsOrImports{JSONValue{Type:String, Value:"./src/things/*/index.js"}}, []string{}, MatchingModePattern, false, false)`。

## 0 直接单测的部分（补充行为级 Rust 测试）

`preferences.go`（偏好矩阵）与 `computeModuleSpecifiers` 等无直接单测；行为由 **P10 auto-import / `.d.ts` emit parity** 兜底。补少量行为级（expected 取自 Go 实现的分支语义）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `allowed_endings_minimal_default` | 默认无偏好 → minimal 优先序 | shortest/无 ending pref + 非 nodenext → `[Minimal, Index, JsExtension]`（无 allowTsExt） | preferences.go:GetAllowedEndingsInPreferredOrder | ✓ |
| `allowed_endings_nodenext_esm_js` | nodenext + ESM 且不允许 ts 扩展 → 只 JsExtension | → `[JsExtension]` | 同 | ✓ |
| `is_excluded_by_regex_match` | 命中排除正则 | specifier `lodash`, excludes `["^lodash$"]` → true | util.go:IsExcludedByRegex | ✓ |
| `count_path_components` | 去前导 `./` 数**分隔符** | `"./a/b/c"` → 2（去 `./` 后 `a/b/c` 含 2 个 `/`；原文档写 3 系误数 component，已按 Go 实测改正）；`"a/b"`→1 | compare.go:CountPathComponents | ✓ |

> **补充：每函数单测覆盖（PORTING §8.6）**。除上述 Go 用例外，本包对每个 `pub fn`/行为不平凡私有 fn
> 都补了行为级 `#[test]`（types `as_str`/`from_str` 往返、util 全部叶函数、preferences 全部 helper、
> lib 引擎 `get_info`/`process_ending`/`try_get_module_name_from_{root_dirs,paths,exports}`/
> `try_get_module_name_as_node_module`/`get_local_module_specifier`/`get_all_module_paths_worker`/
> `get_module_specifiers_for_file_with_info`、util `process_entrypoint_ending`），均 red→green。
> 计：62 个 `#[test]` + 13 个 doctest 全绿。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（6 个）都已映射
- [x] 每个表驱动子用例都已逐行列出（4+1+4+2+3+2 = 16 个 + 嵌套 group）
- [x] expected 值均取自 Go 测试字面量（`count_path_components` 一处按 Go 实测改正了文档笔误）
- [x] 每条带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐：`GetEachFileNameOfModule`/`ContainsNodeModules`/`containsIgnoredPath`/`TryGetRealFileNameForNonJSDeclarationFileName`/`tryGetModuleNameFromExportsOrImports` 实现 TODO 承载这些用例

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `GetModuleSpecifiers` / `computeModuleSpecifiers` 全链路 | 自动导入端到端 | P10（fourslash auto-import） |
| preferences 完整矩阵（relative/non-relative/shortest × endings） | 端到端 | P10 |
| project-reference 相关入口（依赖 `tsoptions`，跨 phase） | 见 impl.md "存疑偏离" | P6/P10 |
