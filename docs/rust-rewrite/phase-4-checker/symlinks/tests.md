# symlinks: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 个测试文件 / 13 个 `func`（8 个 `Test*` + 5 个 `Benchmark*`）/ `TestGuessDirectorySymlink`(5 子用例) + `TestIsNodeModulesOrScopedPackageDirectory`(6 子用例) 等。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/symlinks/knownsymlinks_test.go` | `internal/symlinks/lib.rs`（`#[cfg(test)] mod tests`） | 8 个 `Test*` |
| `internal/symlinks/knownsymlinks_bench_test.go` | （benchmark）→ 仅作 P10 性能对拍 | 5 个 `Benchmark*` |

## `knownsymlinks_test.go`

### `TestNewKnownSymlink`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `new_known_symlink_fields` | 构造器设置 cwd 与大小写敏感 | `new("/test/dir", true)` → `cwd=="/test/dir"`, `use_case_sensitive==true` | `knownsymlinks_test.go:TestNewKnownSymlink` | |

### `TestSetDirectory`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `set_directory_stores_link` | 目录存入后可读回 Real/RealPath | `set_directory("/test/symlink", path, link{Real:"/real/path/", RealPath:...})` → `directories[path]` 的 real/real_path 匹配 | `knownsymlinks_test.go:TestSetDirectory` | |
| `set_directory_realpath_mapping` | realpath→symlink 反查 set 建立 | `directories_by_realpath[real_path]` 含 `"/test/symlink"` | 同 | |

### `TestSetFile`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `set_file_stores_realpath` | 文件 symlink→realpath 存入 | `set_file("/test/symlink/file.ts", path, "/real/path/file.ts")` → `files[path]=="/real/path/file.ts"` | `knownsymlinks_test.go:TestSetFile` | |

### `TestProcessResolution`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `process_resolution_empty_noop` | 任一路径空串则不登记 | `("","")`/`("original","")`/`("","resolved")` → 无副作用 | `knownsymlinks_test.go:TestProcessResolution` | |
| `process_resolution_valid` | 有效解析登记文件映射 | `("/test/original/file.ts","/test/resolved/file.ts")` → `files[path]=="/test/resolved/file.ts"` | 同 | |

### `TestGuessDirectorySymlink`（表驱动，5 子用例）

| Rust 测试 | 验证内容 | input(a,b,cwd) → expected[commonResolved, commonOriginal] | Go 对照 | 完成 |
|---|---|---|---|---|
| `guess_identical_paths` | 同路径 → 退到根 | `("/test/path/file.ts","/test/path/file.ts","/test/dir")` → `("/","/")` | `TestGuessDirectorySymlink/identical paths` | |
| `guess_diff_files_same_dir` | 同目录不同文件名 → 无 symlink | `(".../file1.ts",".../file2.ts",...)` → `("","")` | `.../different files same directory` | |
| `guess_diff_dirs` | 不同目录同文件名 → 各自父目录 | `("/test/path1/file.ts","/test/path2/file.ts",...)` → `("/test/path1","/test/path2")` | `.../different directories` | |
| `guess_node_modules_paths` | node_modules 边界停止弹出 | `("/test/node_modules/pkg/file.ts", 同, ...)` → `("/test/node_modules/pkg","/test/node_modules/pkg")` | `.../node_modules paths` | |
| `guess_scoped_package_paths` | @scope 边界停止弹出 | `("/test/node_modules/@scope/pkg/file.ts", 同, ...)` → `("/test/node_modules/@scope/pkg", 同)` | `.../scoped package paths` | |

### `TestIsNodeModulesOrScopedPackageDirectory`（表驱动，6 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `nm_node_modules` | `node_modules` → true | `"node_modules"` → `true` | `.../node_modules` | |
| `nm_scoped_package` | `@scope` → true | `"@scope"` → `true` | `.../scoped package` | |
| `nm_regular_dir` | 普通目录 → false | `"src"` → `false` | `.../regular directory` | |
| `nm_empty_string` | 空串 → false | `""` → `false` | `.../empty string` | |
| `nm_uppercase_node_modules` | 大写 NODE_MODULES（大小写敏感下）→ false | `"NODE_MODULES"`（cwd 大小写敏感）→ `false` | `.../case insensitive node_modules` | |
| `nm_uppercase_scoped` | `@SCOPE` 仍 true（仅看首字符） | `"@SCOPE"` → `true` | `.../case insensitive scoped` | |

### `TestSetSymlinksFromResolutions`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `set_symlinks_from_resolutions` | 从两条 resolved module 喂入后文件映射建立 | mock 2 个 module（orig/resolved file1/file2）→ `files[path(orig)]==resolved` | `knownsymlinks_test.go:TestSetSymlinksFromResolutions` | |

### `TestKnownSymlinksThreadSafety`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `thread_safety_concurrent_set_directory` | 10 线程并发 set 后全部可读且总数=10 | `std::thread::scope` 并发 `set_directory` ×10 → `directories.len()==10`，各 Real 正确 | `knownsymlinks_test.go:TestKnownSymlinksThreadSafety` | |

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*`（8 个）都已映射
- [ ] 每个表驱动子用例都已逐行列出（`guessDirectorySymlink` 5 + `isNodeModules...` 6）
- [ ] expected 值均取自 Go 测试字面量
- [ ] 每条带 `// Go:` 锚点
- [ ] 与 impl.md 双向对齐：每个公开方法都有用例承载

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `Benchmark*`（5 个：PopulateSymlinksFromResolutions/SetFile/SetDirectory/GuessDirectorySymlink/ConcurrentAccess） | 仅性能；并发对拍 | P10 |
