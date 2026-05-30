# symlinks: 实现方案（impl.md）

**crate**：`tsgo_symlinks`　**目标**：记录"符号链接路径 ↔ 真实路径"的双向映射缓存（文件级 + 目录级），并能从模块解析结果反推目录级 symlink。供 `modulespecifiers`（自动导入要在 symlink 与 realpath 间选更短/更合适的说明符）与 program 使用。
**依赖（crate）**：`tsgo_ast` `tsgo_collections`（SyncMap/SyncSet） `tsgo_core` `tsgo_module` `tsgo_tspath`
**Go 源**：`internal/symlinks/`（1 个非测试文件 `knownsymlinks.go`，135 行）

## 这个包是什么（业务说明）

在 `node_modules` / monorepo / pnpm 场景里，一个文件常常既有"真实路径"（`realpath`，磁盘上的真位置）又有"符号链接路径"（用户工程里看到的位置）。当 TS 给用户生成自动导入说明符时，需要在这两种路径间选择（通常偏好工程内的 symlink 路径而非深埋 `node_modules/.pnpm` 的 realpath）。

`KnownSymlinks` 就是这份双向缓存：
- `directories` / `files`：symlinkPath → realpath。
- `directoriesByRealpath` / `filesByRealpath`：realpath → {symlinkPaths}（一个真实路径可能有多个 symlink 指向它，故用 set）。
- `SetSymlinksFromResolutions` / `ProcessResolution`：从模块解析结果（`module.ResolvedModule.OriginalPath` vs `ResolvedFileName`）增量喂入；`guessDirectorySymlink` 通过比较两条路径的公共后缀，反推出目录级 symlink（跳过 `node_modules` 与 `@scope` 目录）。

它在 Phase 4 因为 import 了 `module`（解析结果类型），且被 `modulespecifiers` import。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包关键决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `KnownSymlinks{ directories SyncMap[Path,*KnownDirectoryLink]; ... }` | `struct KnownSymlinks { directories: DashMap<Path, KnownDirectoryLink>, directories_by_realpath: DashMap<Path, DashSet<String>>, files: DashMap<Path, String>, files_by_realpath: DashMap<Path, DashSet<String>>, cwd: String, use_case_sensitive_file_names: bool }` | `collections.SyncMap` → `dashmap::DashMap`；`SyncSet` → `dashmap::DashSet`（PORTING §3） |
| `*KnownDirectoryLink`（可空） | `KnownDirectoryLink`（值）+ `directories` 存 `Option<KnownDirectoryLink>` | Go `SetDirectory` 可存 nil（表示"已知不是 symlink 目录"）；Rust 用 `Option` |
| `KnownDirectoryLink{ Real string; RealPath tspath.Path }` | `struct KnownDirectoryLink { real: String, real_path: Path }` | `Real`/`RealPath` 都带尾随分隔符（不变量，rustdoc 写明） |
| `forEachResolvedModule func(callback, file)` 高阶回调 | 闭包参数 `for_each_resolved_module: impl Fn(&mut dyn FnMut(&ResolvedModule, &str, ResolutionMode, Path), Option<&SourceFile>)` | Go 的双层回调直译为闭包（PORTING §4 头等函数） |
| `LoadOrStore(realpath, &SyncSet{})` | `directories_by_realpath.entry(rp).or_default()` 后 `.add(symlink)` | `LoadOrStore` → `entry().or_insert_with` |
| 字段大小写敏感比较 `GetCanonicalFileName` | 复用 `tsgo_tspath::get_canonical_file_name` | |

**并发**：4 个 map 是并发安全缓存（program 多线程喂入解析结果）。Rust 用 `DashMap`/`DashSet`，与 Go `SyncMap`/`SyncSet` 对齐（PORTING §6）。`SetDirectory`/`SetFile` 的 "首次见到才登记 realpath set" 逻辑需保持（用 `directories.contains_key` 守卫，注意与 `dashmap` 的原子性——见"已知偏离"）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/symlinks/knownsymlinks.go` | `internal/symlinks/lib.rs` | crate 根。全部类型与方法 |

## 依赖白名单（本包新增的 crate）

- **无新增 crate 依赖**。实现期改用 `tsgo_collections::SyncMap`/`SyncSet`（它们正是 Go `collections.SyncMap`/`SyncSet` 的 1:1 端口，内部已基于 `dashmap`），比直接引 `dashmap` 更贴合 Go 源（`collections.SyncMap[...]` → `tsgo_collections::SyncMap<...>`），且 `tsgo_collections` 本就是已声明的依赖。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `knownsymlinks.go`）

- [x] `pub struct KnownDirectoryLink { real: String, real_path: Path }`（两字段均带尾随分隔符）　`// Go: knownsymlinks.go:KnownDirectoryLink`
- [x] `pub struct KnownSymlinks { 4 个 SyncMap/SyncSet + cwd + use_case_sensitive_file_names }`　`// Go: knownsymlinks.go:KnownSymlinks`
- [x] `pub fn new_known_symlink(current_directory, use_case_sensitive_file_names) -> KnownSymlinks`　`// Go: knownsymlinks.go:NewKnownSymlink`
- [x] `pub fn has_directory(&self, symlink_path: Path) -> bool`（key 先 `ensure_trailing_directory_separator`）　`// Go: knownsymlinks.go:HasDirectory`
- [x] `pub fn directories(&self) -> &SyncMap<Path, Option<KnownDirectoryLink>>`　`// Go: knownsymlinks.go:Directories`
- [x] `pub fn directories_by_realpath(&self) -> &SyncMap<Path, Arc<SyncSet<String>>>`　`// Go: knownsymlinks.go:DirectoriesByRealpath`
- [x] `pub fn files(&self) -> &SyncMap<Path, String>`　`// Go: knownsymlinks.go:Files`
- [x] `pub fn files_by_realpath(&self) -> &SyncMap<Path, Arc<SyncSet<String>>>`　`// Go: knownsymlinks.go:FilesByRealpath`
- [x] `pub fn set_directory(&self, symlink, symlink_path, real_directory: Option<KnownDirectoryLink>)`：仅当 `real_directory` 有值且 `directories` 尚无该 key 时，往 `directories_by_realpath[real_path]` set 加 symlink；最后 `directories.store(symlink_path, real_directory)`　`// Go: knownsymlinks.go:SetDirectory`
- [x] `pub fn set_file(&self, symlink, symlink_path, realpath)`：尚无该 key 时往 `files_by_realpath[to_path(realpath)]` 加 symlink；最后 `files.store`　`// Go: knownsymlinks.go:SetFile`
- [x] `pub fn set_symlinks_from_resolutions(&self, for_each_resolved_module, for_each_resolved_type_reference_directive)`：两个回调里各 `process_resolution(original_path, resolved_file_name)`　`// Go: knownsymlinks.go:SetSymlinksFromResolutions`
- [x] `pub fn process_resolution(&self, original_path, resolved_file_name)`：空串短路；`set_file`；`guess_directory_symlink` 命中且非 ignored path → `set_directory`（real/real_path 均加尾随分隔符）　`// Go: knownsymlinks.go:ProcessResolution`
- [x] `fn guess_directory_symlink(&self, a, b, cwd) -> (String, String)`：把 a/b 拆成 path components，从尾部往前比较——只要两侧倒数第二段都不是 `node_modules`/`@scope` 且最后一段规范名相等就同步弹出；最终若发生过弹出则返回两侧公共目录，否则 `("","")`　`// Go: knownsymlinks.go:guessDirectorySymlink`
- [x] `fn is_node_modules_or_scoped_package_directory(&self, s) -> bool`：`s != "" && (canonical(s)=="node_modules" || s.starts_with('@'))`　`// Go: knownsymlinks.go:isNodeModulesOrScopedPackageDirectory`

### Cargo / crate 接线

- [x] `internal/symlinks/Cargo.toml`（`name = "tsgo_symlinks"` + path deps：`tsgo_ast` `tsgo_collections` `tsgo_core` `tsgo_module` `tsgo_tspath`）— 已 scaffold；无新增 crate 依赖（用 `tsgo_collections::SyncMap`/`SyncSet` 取代 `dashmap`）
- [x] 根 `Cargo.toml` workspace members 追加（已 scaffold）
- [x] `lib.rs` 直接以 crate 根 `pub` 暴露 `KnownSymlinks` / `KnownDirectoryLink` / `new_known_symlink`（无需额外 `pub use`）

## TDD 推进顺序（tracer bullet → 增量）

1. `new_known_symlink` + 字段可见性 → `TestNewKnownSymlink`。
2. `is_node_modules_or_scoped_package_directory`（纯函数表驱动）→ `TestIsNodeModulesOrScopedPackageDirectory`。
3. `guess_directory_symlink`（表驱动）→ `TestGuessDirectorySymlink`。
4. `set_directory` / `set_file`（含 realpath set 反查）→ `TestSetDirectory` / `TestSetFile`。
5. `process_resolution` / `set_symlinks_from_resolutions` → 对应测试。
6. 并发安全 → `TestKnownSymlinksThreadSafety`（rayon/std::thread::scope 多线程并发 `set_directory`）。

## 与 Go 的已知偏离（divergence）

- Go `collections.SyncMap`/`SyncSet` → `tsgo_collections::SyncMap`/`SyncSet`（**而非**直接 `dashmap::DashMap`/`DashSet`；前者是 Go 类型的 1:1 端口、内部即 `dashmap`，映射更直）。
- Go `*collections.SyncSet[string]`（指针，多 key 可共享同一 set）→ `Arc<SyncSet<String>>`（`Arc` 克隆共享内层 set，对应 Go 指针语义；`SyncSet` 自身非 `Clone`）。
- `*KnownDirectoryLink` 可空 → `SyncMap` 值取 `Option<KnownDirectoryLink>`（`None` 表示 Go 的 nil，即"已知不是 symlink 目录"）。
- `NewKnownSymlink` 返回 `*KnownSymlinks`（堆指针）→ `new_known_symlink` 返回 `KnownSymlinks`（值）。
- `SetSymlinksFromResolutions` 的双层高阶回调：Go 的 `func(callback, file)` → Rust `FnOnce(&mut dyn FnMut(&ResolvedModule, &str, ResolutionMode, Path), Option<&SourceFileData>)`；`file` 始终为 `None`（与 Go 传 `nil` 一致），`ast.SourceFile` 在 arena 端口里对应 `tsgo_ast::SourceFileData`。
- **`SetDirectory` 的 check-then-act 原子性**：Go 用 `directories.Load` 守卫再 `Store`，本身在并发下也非严格原子（与 realpath set 登记之间有窗口）；Rust 同样保持 best-effort 语义（不强行加全局锁），与 Go 行为一致。
- 测试 `TestKnownSymlinksThreadSafety` 用 goroutine + channel；Rust 用 `std::thread::scope`（PORTING §6）。该测试验证既有方法的并发安全（emergent property），无新增实现，故直接绿。

## 转交 / 推迟（DEFER）

- 无。依赖的 `tsgo_module`（同 phase 内、依赖序在前）需先收口；`tsgo_collections`/`tsgo_tspath`（P1）已就绪。
