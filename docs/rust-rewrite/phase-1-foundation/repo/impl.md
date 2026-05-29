# repo: 实现方案（impl.md）

**crate**：`tsgo_repo`　**目标**：在**测试期**定位仓库根、TypeScript 子模块路径、testdata 路径，并提供"无子模块则跳过测试"的辅助。
**依赖（crate）**：无（仅标准库 `std::fs` / `std::path` / `std::env`）。叶子包，**仅测试设施**。
**Go 源**：`internal/repo/`（1 个非测试文件：`paths.go` 88 行）

## 这个包是什么（业务说明）

`repo` 不参与编译器运行时，纯粹是**测试基础设施**：很多测试需要读 `_submodules/TypeScript`（TS 官方子模块，存放 conformance 语料）或 `testdata`。本包用 `runtime.Caller(0)` 拿到自身源码路径，向上找 `go.mod` 定位仓库根，再拼出各路径；并用 `sync.OnceValue` 缓存（只算一次）。

- `RootPath()`：从 `paths.go` 自身位置向上找 `go.mod` 的目录。`-trimpath` 构建下会 panic（路径被裁剪）。
- `TypeScriptSubmodulePath()` = `<root>/_submodules/TypeScript`。
- `TestDataPath()` = `<root>/testdata`。
- `TypeScriptSubmoduleExists()`：检查子模块 `package.json` 是否存在。
- `SkipIfNoTypeScriptSubmodule(t)`：测试辅助，子模块不存在则 `t.Skipf`。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `runtime.Caller(0)` 取源文件路径 | `env!("CARGO_MANIFEST_DIR")` / `file!()` | **关键偏离**：Rust 用编译期宏 `CARGO_MANIFEST_DIR` 定位 crate 目录，比向上找 `go.mod` 更直接 |
| 向上找 `go.mod` | 向上找 `Cargo.toml`（workspace 根标记） 或直接用 manifest dir | 找 workspace 根可用 `CARGO_MANIFEST_DIR` 向上 / `CARGO_WORKSPACE_DIR`（不稳定）/ 找含 `[workspace]` 的 `Cargo.toml` |
| `sync.OnceValue(fn)` | `std::sync::OnceLock<String>` + 闭包 | 一次性计算缓存 |
| `filepath.Join` / `filepath.Dir` / `VolumeName` | `std::path::Path::join` / `parent` / `PathBuf` | 路径操作 |
| `os.Stat` + `os.IsNotExist` | `std::path::Path::exists` / `std::fs::metadata` | 存在性检查 |
| `SkippableTest` 接口（`Helper`/`Skipf`） | 测试侧用 `return`/宏跳过；或 `#[ignore]` + 运行时判定 | Rust 无 `t.Skip`；可用 `if !exists() { return; }` 或自定义 skip 宏 |
| `panic(...)`（trimpath / 找不到 go.mod） | `panic!(...)` | 同语义 |

> **核心偏离**：Go 靠 `runtime.Caller` + `go.mod` 在运行时反推仓库根；Rust 用编译期 `env!("CARGO_MANIFEST_DIR")` 即可拿到 `internal/repo` 目录，再 `.parent().parent()` 上溯到仓库根。这避免了运行时反射式定位，结构更稳。`-trimpath` 对应的 panic 在 Rust 通常不需要（manifest dir 是编译期常量）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/repo/paths.go` | `internal/repo/paths.rs`（在 `lib.rs` 里 `mod paths; pub use paths::*;`） | 路径定位 + skip 辅助 |
| （crate 根） | `internal/repo/lib.rs` | `tsgo_repo` 入口 |

## 依赖白名单（本包新增的 crate）

- 无。仅标准库。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `paths.rs`（Go: `internal/repo/paths.go`）

- [x] `pub fn root_path() -> &'static str` — `OnceLock`；用 `CARGO_MANIFEST_DIR` 上溯到 workspace 根（含 `[workspace]` 的 `Cargo.toml`）；找不到 → panic　`// Go: paths.go:RootPath/rootPath`
- [x] `pub fn typescript_submodule_path() -> &'static str` — `<root>/_submodules/TypeScript`　`// Go: paths.go:TypeScriptSubmodulePath`
- [x] `pub fn test_data_path() -> &'static str` — `<root>/testdata`　`// Go: paths.go:TestDataPath`
- [x] `pub fn typescript_submodule_exists() -> bool` — 检查 `<submodule>/package.json` 存在　`// Go: paths.go:TypeScriptSubmoduleExists`
- [x] `pub fn skip_if_no_typescript_submodule() -> bool`（或宏 `skip_if_no_ts_submodule!()`） — 子模块缺失时让测试跳过　`// Go: paths.go:SkipIfNoTypeScriptSubmodule`

### Cargo / crate 接线

- [x] `internal/repo/Cargo.toml`（`name = "tsgo_repo"`）
- [x] 根 `Cargo.toml` workspace members 追加
- [x] `lib.rs` 声明 `mod paths;` + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `root_path` 返回的目录里确实有 workspace `Cargo.toml`（tracer bullet）。
2. `typescript_submodule_path` / `test_data_path` 拼接正确。
3. `typescript_submodule_exists` 与磁盘实况一致。
4. skip 辅助：子模块缺失时测试被跳过（不失败）。

## 与 Go 的已知偏离（divergence）

- **定位机制**：`runtime.Caller` + 找 `go.mod` → 编译期 `CARGO_MANIFEST_DIR` + 找 workspace `Cargo.toml`。等价目标（拿到仓库根），实现更稳。
- **`-trimpath` panic**：Rust manifest dir 为编译期常量，通常无需此分支；若用 `file!()` 路径反推则需保留类似保护。本包采用 manifest dir 方案，标 `// PERF(port)` 说明取舍。
- **测试 skip**：Go `t.Skipf` 无直接对应；Rust 用提前 `return` + 日志，或自定义 skip 宏。需保证缺子模块时 CI 不红。

## 转交 / 推迟（DEFER）

- 实际依赖本包的测试（conformance/testdata 驱动）集中在 P10；本包只需在 P1 提供稳定路径 API。
