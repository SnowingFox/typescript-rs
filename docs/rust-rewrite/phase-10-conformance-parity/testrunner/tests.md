# testrunner: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：3 个 `*_test.go` 文件 / 4 个 `func Test*`（其中 `TestMain` 是测试入口）/ 1 个有具体 input-expected 的单测（`TestMakeUnitsFromTest`）。

> `testrunner` 的"测试"分两类：(1) `TestMakeUnitsFromTest` 是真正的纯逻辑单测，逐字段对齐；(2) `TestLocal` / `TestSubmodule` 是**驱动器入口**——它们本身不带断言字面量，而是遍历 `testdata/tests/cases` 跑编译、由 `baseline.Run` 与 reference 逐字节对拍。后两者的"通过"= conformance baseline 全绿（详细分批口径见 [conformance-parity/tests.md](../conformance-parity/tests.md)）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/testrunner/test_case_parser_test.go` | `internal/testrunner/test_case_parser.rs`（`#[cfg(test)] mod tests`） | 1（`TestMakeUnitsFromTest`） |
| `internal/testrunner/compiler_runner_test.go` | `internal/testrunner/compiler_runner.rs`（`#[cfg(test)] mod tests`）或集成测试 | 2（`TestLocal` / `TestSubmodule`） |
| `internal/testrunner/testmain_test.go` | harness 入口（无 Rust `#[test]`，对应 `TestMain` 的 setup/teardown） | 1（`TestMain`） |

## `test_case_parser_test.go`

### `TestMakeUnitsFromTest`（逐字段对齐，唯一纯逻辑单测）

> Go: 给定一段含两个 `// @filename` 的源码，验证切成两个 `testUnit`，且**注释归属正确**（普通注释跟在前一文件，`@filename` 指令本身不进内容）。input/expected 取自 Go 测试字面量。

input（`code`，文件名 `simpleTest.ts`）：
```
// @strict: true
// @noEmit: true
// @filename: firstFile.ts
function foo() { return "a"; }
// normal comment
// @filename: secondFile.ts
// some other comment
function bar() { return "b"; }
```

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `make_units_two_files` | 切出 2 个 unit，名正确 | 上述 code → `unit[0].name == "firstFile.ts"`, `unit[1].name == "secondFile.ts"` | `test_case_parser_test.go:TestMakeUnitsFromTest` | |
| `make_units_first_content` | 第 1 文件含其后普通注释，不含 `@filename` 行 | → `unit[0].content == "function foo() { return \"a\"; }\n// normal comment"` | 同上 | |
| `make_units_second_content` | 第 2 文件含其前导注释 | → `unit[1].content == "// some other comment\nfunction bar() { return \"b\"; }"` | 同上 | |
| `make_units_global_options_excluded` | `@strict`/`@noEmit` 是全局 option，不进任何 unit 内容；`tsConfig`/`symlinks` 为空/默认 | → `ts_config == None`, `symlinks` 空 map | 同上（`testCaseContent` 全字段 DeepEqual） | |

> 实现注意：Go 用 `cmp.AllowUnexported` 对整个 `testCaseContent` DeepEqual；Rust 让 `TestUnit`/`TestCaseContent` derive `PartialEq` 后单次 `assert_eq!` 即可覆盖上面 4 行（拆成 4 个断言只为可读，可合并成 1 个 `#[test]`）。

### 补充：`parse_test_files_and_symlinks_with_options`（fourslash 路径行为测试）

> Go 无直接单测，但 fourslash 阻塞依赖 `AllowImplicitFirstFile` 分支。补行为级测试（expected 取自对 Go 代码路径的推演 + fourslash 实际用法）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `parse_implicit_first_file` | 无 `@filename` 时内容进隐式首文件 | `"var x;"` + allow_implicit + fileName="a.ts" → 1 unit name="a.ts" | Go `ParseTestFilesAndSymlinksWithOptions`（AllowImplicitFirstFile 分支） | |
| `parse_symlink_directive` | `// @link: A -> B` 进 symlinks | 含 `@link` → `symlinks["B"]=="A"` | Go `parseSymlinkFromTest` | |
| `parse_symlink_option` | `// @symlink: X` 关联当前文件 | `@symlink` → symlinks 记录 | Go 行 ~170 | |
| `parse_emitthisfile_directive` | `emitthisfile` 是文件级 directive | `@emitThisFile: true` → 文件 option 而非全局 | Go `fourslashDirectives` | |

### 补充：`extract_compiler_settings`

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `extract_settings_lowercase_key` | key 小写 + 去尾分号 | `// @Strict: true;` → `{"strict": "true"}` | Go `extractCompilerSettings`（ToLower + TrimSuffix ";") | |

## `compiler_runner_test.go`

> 这两个是**驱动器入口**，不带断言字面量。它们的语义是"枚举所有 fixture → 跑编译 → 与 reference 对拍"。Rust 侧对齐为：实现等价的入口函数 + duplicate-test-name 自检，实际"绿/红"由 conformance-parity 分批 checklist 决定。

| Rust 测试 / 入口 | 验证内容 | 行为 | Go 对照 | 完成 |
|---|---|---|---|---|
| `test_local` | 跑 corsa 自带 `testdata/tests/cases`（compiler + conformance），产 local baseline 并与 reference 比 | `run_compiler_tests(h, is_submodule=false)`：跳过无 bundled、duplicate test name 自检、两个 runner（Regression + Conformance）依次 `run_tests` | `compiler_runner_test.go:TestLocal` | — (分批，见 conformance-parity) |
| `test_local_no_duplicate_test_files` | 两个 runner 的文件名集合无重复 | 枚举 compiler+conformance basename → `assert !seen.contains` | `compiler_runner_test.go:runCompilerTests`（seenTests 自检） | |
| `test_submodule` | 跑 TS 原始 submodule cases，产 diff baseline | `run_compiler_tests(h, is_submodule=true)`：无 submodule 时 skip | `compiler_runner_test.go:TestSubmodule` | — (DEFER：submodule checkout) |
| `skip_when_not_bundled` | 未嵌入 bundled lib 时 skip | `if !bundled.Embedded { skip }` | `compiler_runner_test.go:runCompilerTests` 行 ~27 | |

## `testmain_test.go`

| Rust 对应 | 验证内容 | 行为 | Go 对照 | 完成 |
|---|---|---|---|---|
| harness setup/teardown | 启用 baseline tracking + debug stack limit | `core::apply_debug_stack_limit()` + `defer baseline::track()()` | `testmain_test.go:TestMain` | |

> Rust 无 `TestMain`；等价物是 harness 的全局 setup（启用 `baseline::track`）与 teardown（写 tracking 文件）。非对拍主路径，简化实现即可。

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*` 都已映射：`TestMakeUnitsFromTest`（→ 4 行）/ `TestLocal`（→ 入口 + 重名自检 + bundled skip）/ `TestSubmodule`（→ 入口，DEFER）/ `TestMain`（→ harness setup）
- [ ] `TestMakeUnitsFromTest` 的表驱动？Go 此用例非表驱动（单一 input），已拆成 4 条断言行（可合并）
- [ ] expected 值均取自 Go 测试字面量（firstFile/secondFile 内容逐字节）
- [ ] 每条带 `// Go:` 锚点
- [ ] 与 impl.md 双向对齐：impl.md 的 `make_units_from_test` / `parse_test_files_and_symlinks_with_options` / `extract_compiler_settings` / `CompilerBaselineRunner` 入口在此都有测试行；本文件每条用例都有 impl.md 承载

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `TestLocal` 全量开绿 | 需 testutil 全部 `Do*Baseline` + 整条 P1-P9 管线就绪；按 conformance 子目录/baseline 类型分批 | conformance-parity |
| `TestSubmodule` | 依赖 TS submodule checkout + 三档 diff baseline | conformance-parity / DEFER |
| 8 个 `verify_*` 的逐类正确性 | 由对应类型的 conformance reference 文件逐字节兜底（不是孤立单测） | conformance-parity |
