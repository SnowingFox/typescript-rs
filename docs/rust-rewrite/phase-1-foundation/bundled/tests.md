# bundled: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 文件 / 2 `func Test` / 2 子用例（无表驱动，每个 func 一条直测）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/bundled/bundled_test.go` | `internal/bundled/lib.rs`（`#[cfg(test)] mod tests`）或 `internal/bundled/tests/embedded.rs` | 2 |

> Go 测试是 `package bundled_test`（黑盒外部测试），用公共 API（`TestingLibPath`/`WrapFS`/`LibPath`/`LibNames`）。Rust 侧放 `tests/embedded.rs` 集成测试更贴近原意；也可放 `#[cfg(test)] mod tests`。`assert.DeepEqual`/`assert.NilError` → `assert_eq!` / `unwrap()`。

## `bundled_test.go`

> 两个 func 都是无表驱动的直测；`t.Parallel()` → Rust 测试天然并行（cargo test 默认多线程），无需特别处理。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `testing_lib_path_has_lib_dts` | `testing_lib_path()` 指向的目录存在，且其下 `lib.d.ts` 存在 | `testing_lib_path()` → 目录可 `stat`；`<p>/lib.d.ts` 可 `stat` | `bundled_test.go:TestTestingLibPath` | |
| `embedded_libs_walk_matches_lib_names` | 用 `wrap_fs(osvfs())` 对 `lib_path()` 做 `walk_dir`，收集所有非目录项的 basename，应**逐项等于** `LIB_NAMES` | `walk_dir(lib_path())` 收集 basenames → `== LIB_NAMES`（110 项，按名序） | `bundled_test.go:TestEmbeddedLibs` | |

### 关键断言细节（取自 Go 实测）

- `TestTestingLibPath`：`os.Stat(p)` 与 `os.Stat(filepath.Join(p,"lib.d.ts"))` 都 `NilError`。Rust：`std::fs::metadata` 均 `Ok`。
- `TestEmbeddedLibs`：遍历用 `tspath.GetBaseFileName(path)` 取 basename，过滤 `d.IsDir()`，最终 `assert.DeepEqual(files, bundled.LibNames)`。**顺序敏感**——`LIB_NAMES`（生成时按 `target` 名 `strings.Compare` 排序）必须与 `walk_dir` 的内嵌目录项遍历顺序一致。这条同时验证了：
  - 内嵌 FS 的 `walk_dir` 行为；
  - `LIB_NAMES` 生成顺序；
  - `LIBS_ENTRIES` 顺序（`walk_dir` 用它枚举 `libs`）。

## 补充的行为级 Rust 测试（公开接口，expected 取自 Go 行为 / 已知值）

Go 侧只有 2 个直测，但本包还有大量未被直测的行为（被 cmd/tsgo/lsp/api 间接依赖）。补少量行为级测试守住内嵌 FS 的关键路径（这些是 `WrappedFs` 的语义，expected 来自 `embed.go` 源码逻辑）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `read_file_returns_embedded_content` | `read_file("bundled:///libs/lib.d.ts")` 命中内嵌内容 | path → `(content, true)`，content == `EMBEDDED_CONTENTS["libs/lib.d.ts"]` | `embed.go:ReadFile` | |
| `file_exists_for_embedded` | 内嵌存在/不存在判定 | `"bundled:///libs/lib.d.ts"`→true；`"bundled:///libs/nope.d.ts"`→false | `embed.go:FileExists` | |
| `directory_exists_only_libs` | 仅 `libs` 目录为真 | `"bundled:///libs"`→true；`"bundled:///other"`→false | `embed.go:DirectoryExists` | |
| `get_accessible_entries_root_and_libs` | 根列出 `["libs"]`；`libs` 列出全部 lib 文件 | `"bundled:///"`→dirs=["libs"]；`"bundled:///libs"`→files==LIB_NAMES | `embed.go:GetAccessibleEntries` | |
| `stat_embedded_dir_and_file` | dir 项 `is_dir()`；file 项 size==len | `"bundled:///libs"`→dir；`"bundled:///libs/lib.d.ts"`→size==content.len | `embed.go:Stat` | |
| `realpath_embedded_is_identity` | 内嵌路径 realpath 原样 | `"bundled:///libs/lib.d.ts"` → 自身 | `embed.go:Realpath` | |
| `is_bundled_true_for_scheme` | scheme 前缀判定 | `"bundled:///x"`→true；`"/abs/x"`→false | `embed.go:IsBundled` | |
| `write_to_embedded_panics` | 写内嵌 FS panic | `write_file("bundled:///libs/x")` → `#[should_panic]` | `embed.go:WriteFile` | |
| `non_bundled_path_delegates` | 非 scheme 路径转发内层 FS | `read_file("/real/x")` → 调内层 | `embed.go:*` 透传分支 | |
| `is_bundled_false_in_noembed`（`--no-default-features`） | noembed 下恒 false | 任意 path → false | `noembed.go:IsBundled` | |

> 这些补充测试用 `vfstest`（内存 FS，P1）作内层 FS，验证 scheme 路由 + 透传两条分支。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（`TestTestingLibPath`→`testing_lib_path_has_lib_dts`；`TestEmbeddedLibs`→`embedded_libs_walk_matches_lib_names`）。
- [x] 无表驱动子用例可列（2 个 func 各 1 条）。
- [x] expected 取自 Go 测试字面量（目录/文件存在性、`LibNames` 全等）。
- [x] 每条带 `// Go:` 锚点。
- [x] 与 impl.md 双向对齐：`testing_lib_path`/`wrap_fs`/`lib_path`/`LIB_NAMES`/`walk_dir`/`read_file`/... 均在 impl.md 有承载 TODO。

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `generate-libs` 再生成产物的端到端校验（对拍 TS 子模块 `src/lib`） | 需 TS 子模块 + 全量 lib 内容；属生成器正确性，非运行时行为 | P10（或随 generate 手动验证） |
| 内嵌 lib 内容被 checker 实际加载并参与类型检查 | 需完整编译管线（P4 checker、P6 compiler） | P10 conformance parity |
