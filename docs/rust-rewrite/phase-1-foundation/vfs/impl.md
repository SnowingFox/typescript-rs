# vfs: 实现方案（impl.md）

**crate**：`tsgo_vfs`（含多个子 module）　**目标**：文件系统抽象（`FS` trait）+ 多种实现：真实磁盘（osvfs）、内存测试 FS（vfstest）、缓存包装（cachedvfs）、依赖追踪包装（trackingvfs）、方法替换包装（wrapvfs）、glob 文件匹配（vfsmatch）、轮询文件监视（vfswatch），以及 mock。
**依赖（crate）**：`tsgo_tspath`、`tsgo_core`、`tsgo_collections`。外部：`dashmap`（缓存）、`xxhash-rust`（vfswatch hash）。
**Go 源**：`internal/vfs/`（20 个非测试文件，含 9 个嵌套子包）

## 这个包是什么（业务说明）

`vfs` 把"文件系统"抽象成一个 `FS` 接口，让编译器/语言服务可以在真实磁盘、内存（测试）、缓存层之间切换。这是编译器与 OS 之间的唯一边界，所有路径都用 tspath 的 `/`-规范化绝对路径。

子包职责：
- **根 `vfs`**：`FS` 接口（`FileExists`/`ReadFile`/`WriteFile`/`Stat`/`WalkDir`/`Realpath`/`GetAccessibleEntries` 等 13 方法）、`Entries`（files/dirs/symlinks）、错误别名、`WalkDirFunc`。
- **`internal`**：`Common`——osvfs/iovfs 共享的实现（路径拆根 `SplitPath`/`RootLength`、`Stat`/`ReadFile`/`WalkDir`/`GetAccessibleEntries`、BOM 解码 UTF-8/UTF-16）。
- **`iovfs`**：把 Go `io/fs.FS` 适配成 `vfs.FS`（`From`）；支持可选 `RealpathFS`/`WritableFS`。
- **`osvfs`**：真实磁盘实现；带并发限流信号量（读 128/写 32/阻塞 128）、大小写敏感性探测、realpath（平台特化）、reparse point（Windows）。
- **`cachedvfs`**：用 `SyncMap` 缓存 `DirectoryExists`/`FileExists`/`GetAccessibleEntries`/`Realpath`/`Stat`（注意 `ReadFile`/写操作不缓存）；可启用/禁用/清空。
- **`trackingvfs`**：记录所有读类访问路径（watch 模式依赖）；写操作不追踪。
- **`wrapvfs`**：用 `Replacements`（一组可选闭包）覆盖底层 FS 的方法。
- **`vfsmatch`**：tsconfig `include`/`exclude` 的 glob 匹配（无正则的自写匹配器 + 目录遍历 + `node_modules`/隐藏文件跳过 + `.min.js` 默认排除）。**最复杂**。
- **`vfstest`**：内存 `MapFS`（基于 `fstest.MapFS`），支持符号链接、大小写敏感性、写、realpath，供测试用。
- **`vfswatch`**：轮询式文件监视（mtime + 目录 children hash），带 debounce。
- **`vfsmock`**：由 moq 生成的 `FSMock` + `Wrap` 辅助（测试用）。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type FS interface { ... }` | `trait Fs { ... }` | 13 方法 trait；实现者用 struct |
| `io/fs.FS` / `fs.DirEntry` / `fs.FileInfo` / `fs.WalkDirFunc` | **无 Rust 标准对应** | Go 的 `io/fs` 抽象在 Rust 无内建；见核心偏离 |
| `iovfs.From(fs.FS)` | 重新设计：内存 FS 直接实现 `Fs` trait | Rust 无 `io/fs`，`iovfs`+`vfstest`+`internal` 的内存路径合并为一个自写的内存 FS |
| `unsafe.String`（ReadFile 零拷贝） | `String::from_utf8(bytes)` | 去 unsafe（PORTING 要零 unsafe），BOM 解码后构造 String |
| BOM 解码（UTF-8/UTF-16 LE/BE） | 手写：检测 `FE FF`/`FF FE`/`EF BB BF` + `encoding_rs`/`char::decode_utf16` | `decodeUtf16` → `char::decode_utf16`；UTF-8 BOM 剥离 |
| `collections.SyncMap`（cachedvfs 缓存） | `dashmap::DashMap` | 并发缓存 |
| `atomic.Bool`（cachedvfs enabled） | `AtomicBool` | |
| `core.NewLimitedSemaphore`（osvfs 限流） | `tsgo_core::Semaphore`（已移植） | 读/写/阻塞信号量 |
| `os.DirFS`/`fs.Sub`/`fs.Stat`/`fs.ReadDir`/`fs.WalkDir` | `std::fs`（`read_dir`/`metadata`/`symlink_metadata`/`read`）+ 自写遍历 | osvfs 直接用 std::fs；WalkDir 自写或 `walkdir` crate |
| `realpath`（平台特化 darwin/linux/windows/other） | `std::fs::canonicalize` + 平台特化（`#[cfg(target_os)]`） | realpath 各平台文件 → `#[cfg]` 分支 |
| reparse point（Windows junction） | `#[cfg(windows)]` 模块 | Windows 特化 |
| `xxh3`（vfswatch children hash） | `xxhash-rust`（xxh3） | 目录列表哈希 |
| `fstest.MapFS`（vfstest 基础） | 自写内存树（`HashMap<canonicalPath, FileNode>`） | Rust 无 `fstest.MapFS`，自写内存 FS |
| `time.Time`（mtime） | `std::time::SystemTime` | |
| moq 生成 `FSMock` | `mockall` 或手写 trait mock | 测试 mock |

### 核心偏离：Go `io/fs` 抽象缺失

Go 的 vfs 层大量依赖 `io/fs.FS`（`os.DirFS`/`fs.Sub`/`fstest.MapFS`），`iovfs.From` 把任意 `io/fs.FS` 适配成 `vfs.FS`，`internal.Common` 在 `io/fs.FS` 之上实现通用逻辑。Rust 标准库**没有** `io/fs.FS` 这一抽象。移植策略：
- **`Fs` trait 直接定义全部 13 方法**（不经 `io/fs` 中间层）。
- **osvfs** 直接基于 `std::fs` 实现 `Fs`。
- **vfstest 的内存 MapFS** 自写一棵内存树实现 `Fs`（吸收 `iovfs`+`internal`+`vfstest` 三者在内存路径上的职责）。
- `internal.Common` 的"按根拆分 + 委派子 FS"逻辑只在需要"挂载多个根"时保留；多数情况下 osvfs/内存 FS 直接处理绝对路径。

这是结构性偏离，须在本 impl.md 顶部记录；行为对齐由 vfstest/iovfs 的单测 gate。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/vfs/vfs.go` | `internal/vfs/lib.rs` | `Fs` trait + `Entries` + 错误 |
| `internal/vfs/internal/internal.go` | `internal/vfs/internal/mod.rs` | `Common` 拆根 + BOM 解码（按需保留） |
| `internal/vfs/iovfs/iofs.go` | `internal/vfs/iovfs/mod.rs` | io/fs 适配（重设计，见偏离） |
| `internal/vfs/osvfs/os.go` + `realpath_*.go` + `reparsepoint_*.go` + `eintr_unix.go` | `internal/vfs/osvfs/mod.rs` + `#[cfg]` 平台文件 | 真实磁盘 |
| `internal/vfs/cachedvfs/cachedvfs.go` | `internal/vfs/cachedvfs/mod.rs` | 缓存包装 |
| `internal/vfs/trackingvfs/trackingvfs.go` | `internal/vfs/trackingvfs/mod.rs` | 追踪包装 |
| `internal/vfs/wrapvfs/wrapvfs.go` | `internal/vfs/wrapvfs/mod.rs` | 替换包装 |
| `internal/vfs/vfsmatch/vfsmatch.go` + `stringer_generated.go` | `internal/vfs/vfsmatch/mod.rs` | glob 匹配（最大） |
| `internal/vfs/vfstest/vfstest.go` | `internal/vfs/vfstest/mod.rs` | 内存测试 FS |
| `internal/vfs/vfswatch/vfswatch.go` | `internal/vfs/vfswatch/mod.rs` | 轮询监视 |
| `internal/vfs/vfsmock/mock_generated.go` + `wrapper.go` | `internal/vfs/vfsmock/mod.rs` | mock + Wrap |

> 子包默认作 `tsgo_vfs` crate 的子 module（PORTING §2）；`osvfs`/`vfsmatch` 若需独立外部依赖可拆独立子 crate，由实现期定。

## 依赖白名单（本包新增的 crate）

- `dashmap`（cachedvfs 缓存）、`xxhash-rust`（vfswatch hash）、`encoding_rs` 或标准 `char::decode_utf16`（BOM/UTF-16 解码）。
- `walkdir`（可选，osvfs WalkDir）、`mockall`（可选，vfsmock）。
- 内部：`tsgo_tspath` `tsgo_core` `tsgo_collections`。
- 记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### 根 `lib.rs`（Go: `vfs.go`）

- [ ] `trait Fs`（13 方法：`use_case_sensitive_file_names / file_exists / read_file -> Option<String> / write_file / append_file / remove / chtimes / directory_exists / get_accessible_entries -> Entries / stat -> Option<FileInfo> / walk_dir / realpath`）　`// Go: vfs.go:FS`
- [ ] `struct Entries{ files: Vec<String>, directories: Vec<String>, symlinks: Option<HashSet<String>> }`　`// Go: vfs.go:Entries`
- [ ] 错误类型/别名 `ErrNotExist` 等 + `WalkDirFunc` + `SkipAll`/`SkipDir`　`// Go: vfs.go`

### `internal`（Go: `internal/internal.go`）

- [ ] `root_length(p) -> usize`（非绝对 panic `vfs: path %q is not absolute`）/ `split_path(p) -> (root, rest)`　`// Go: internal.go:RootLength/SplitPath`
- [ ] `Common`（`stat/file_exists/directory_exists/get_accessible_entries/get_entries/walk_dir/read_file`，按根拆分 + 子 FS 委派）（按偏离取舍保留）　`// Go: internal.go:Common.*`
- [ ] `decode_bytes`（BOM：UTF-16 LE/BE/UTF-8）+ `decode_utf16`　`// Go: internal.go:decodeBytes/decodeUtf16`

### `osvfs`（Go: `osvfs/os.go` + 平台文件）

- [ ] `OsFs` 实现 `Fs`（读/写/stat/walk 经信号量限流）　`// Go: os.go:osFS.*`
- [ ] `is_file_system_case_sensitive`（启动探测：Windows→false、其它→可执行文件大小写翻转 stat）+ `swap_case`　`// Go: os.go:isFileSystemCaseSensitive/swapCase`
- [ ] `realpath`（平台特化 `#[cfg]` darwin/linux/windows/other）+ `os_fs_realpath`　`// Go: os.go:Realpath/osFSRealpath + realpath_*.go`
- [ ] reparse point 判定（`#[cfg(windows)]`）+ 其它平台空实现　`// Go: reparsepoint_windows.go/reparsepoint_other.go`
- [ ] `write_file`/`append_file`（带"先写失败则建目录再写"）+ `remove`(RemoveAll) + `chtimes`　`// Go: os.go:writeFileEnsuringDir/Remove/Chtimes`
- [ ] `get_global_typings_cache_location`（UserCacheDir + 平台子目录 + version）　`// Go: os.go:GetGlobalTypingsCacheLocation`
- [ ] 信号量：读 128 / 写 32 / 阻塞 128（用 `tsgo_core::LimitedSemaphore`）　`// Go: os.go:blockingOpSema/readSema/writeSema`

### `cachedvfs`（Go: `cachedvfs/cachedvfs.go`）

- [ ] `CachedFs`（包装 `Fs` + `enabled: AtomicBool` + 5 个 `DashMap` 缓存）　`// Go: cachedvfs.go:FS`
- [ ] `from / enable / disable_and_clear_cache / clear_cache`　`// Go: cachedvfs.go:From/Enable/DisableAndClearCache/ClearCache`
- [ ] 缓存方法：`directory_exists/file_exists/get_accessible_entries/realpath/stat`（启用时查缓存→miss 调底层→存）；**`read_file`/`write_file`/`append_file`/`remove`/`chtimes`/`walk_dir`/`use_case_sensitive_file_names` 直接透传不缓存**　`// Go: cachedvfs.go:*`

### `trackingvfs`（Go: `trackingvfs/trackingvfs.go`）

- [ ] `TrackingFs{ inner, seen_files: SyncSet<String> }`；读类方法（read_file/file_exists/directory_exists/get_accessible_entries/stat/walk_dir）记录路径；写类不记录　`// Go: trackingvfs.go:*`

### `wrapvfs`（Go: `wrapvfs/wrapvfs.go`）

- [ ] `Replacements`（13 个 `Option<Box<dyn Fn...>>`）+ `wrap(fs, replacements)` + `WrappedFs`（有 replacement 用之，否则委派）　`// Go: wrapvfs.go:*`

### `vfsmatch`（Go: `vfsmatch/vfsmatch.go` + stringer）—— 最大

- [ ] `enum Usage{Files,Directories,Exclude}` + Stringer + `UNLIMITED_DEPTH`　`// Go: vfsmatch.go:Usage/UnlimitedDepth`
- [ ] `read_directory(host, current_dir, path, extensions, excludes, includes, depth) -> Vec<String>`（入口）　`// Go: vfsmatch.go:ReadDirectory/matchFiles`
- [ ] `is_implicit_glob(last_component) -> bool`　`// Go: vfsmatch.go:IsImplicitGlob`
- [ ] `get_include_base_path` / `get_base_paths`（去重非通配 base）　`// Go: vfsmatch.go:getIncludeBasePath/getBasePaths`
- [ ] glob 编译：`GlobPattern{components, is_exclude, case_sensitive, exclude_min_js}` + `Component{kind, literal, segments, skip_package_folders}` + `Segment{kind, literal}` + `compile_glob_pattern` + `parse_component` + `parse_segments`　`// Go: vfsmatch.go:globPattern/component/segment/compileGlobPattern/parseComponent/parseSegments`
- [ ] 匹配：`matches / matches_parts / matches_prefix_parts / match_path_parts`（kindDoubleAsterisk/Literal/Wildcard）+ `pattern_satisfied`　`// Go: vfsmatch.go:matches/matchPathParts/patternSatisfied`
- [ ] 路径段扫描：`next_path_part_single` / `next_path_part_parts`（prefix+suffix 虚拟拼接）　`// Go: vfsmatch.go:nextPathPartSingle/nextPathPartParts`
- [ ] 通配段匹配：`match_wildcard`（快路径 `*literal`）+ `match_segments`（迭代 + 单星回溯，O(n*m)，rune 推进）　`// Go: vfsmatch.go:matchWildcard/matchSegments`
- [ ] `.min.js` 处理：`should_include_min_js / has_min_js_suffix / pattern_mentions_min_suffix`　`// Go: vfsmatch.go:*`
- [ ] 辅助：`strings_equal`（大小写）/ `is_hidden_path` / `is_package_folder`（node_modules/jspm_packages/bower_components）/ `ensure_trailing_slash`　`// Go: vfsmatch.go:*`
- [ ] `GlobMatcher{includes, excludes, had_includes}` + `new_glob_matcher` + `matches_file_parts -> (usize, bool)` + `matches_directory_parts`　`// Go: vfsmatch.go:globMatcher/*`
- [ ] `GlobVisitor`（目录遍历 + symlink 环检测 via Realpath/canonical + 增量 realpath + 扩展名过滤 + depth）`visit` + `match_files`　`// Go: vfsmatch.go:globVisitor.visit/matchFiles`
- [ ] `SpecMatcher{patterns}` + `match_string / match_index / new_spec_matcher`　`// Go: vfsmatch.go:SpecMatcher/*`

### `vfstest`（Go: `vfstest/vfstest.go`）

- [ ] 内存 `MapFs`（canonical path → 文件节点 + symlinks 表 + clock）实现 `Fs` + `RealpathFs`/`WritableFs`　`// Go: vfstest.go:MapFS`
- [ ] `from_map / from_map_with_clock`（路径校验：rooted/normalized/posix-xor-windows，文件类型 string/bytes/MapFile）+ `convert_map_fs`（建中间目录 + 去重 canonical 冲突 panic）　`// Go: vfstest.go:FromMap/FromMapWithClock/convertMapFS`
- [ ] symlink：`Symlink(target)` + `get_following_symlinks(_worker)`（链式跟随 + 目录下符号链接 + broken symlink error）　`// Go: vfstest.go:Symlink/getFollowingSymlinks*`
- [ ] `mkdir_all`（含 symlinked parent 处理）/ `write_file` / `append_file` / `remove`（递归删 + 删 symlink 记录）/ `chtimes`　`// Go: vfstest.go:mkdirAll/WriteFile/AppendFile/remove/Chtimes`
- [ ] 辅助：`get_canonical_path / set_entry / dir_name / base_name / compare_paths_by_parts / add_symlink / get_target_of_symlink / get_mod_time / entries / get_file_info`　`// Go: vfstest.go:*`
- [ ] Clock trait（`now`/`since_start`）+ 默认实现　`// Go: vfstest.go:Clock/clockImpl`

### `vfswatch`（Go: `vfswatch/vfswatch.go`）

- [ ] `FileWatcher{ fs, poll_interval, testing, callback, watch_state, wildcard_directories, mu: Mutex, debug_log }` + `WatchEntry{mod_time, exists, children_hash}`　`// Go: vfswatch.go:FileWatcher/WatchEntry`
- [ ] `new_file_watcher / set_debug_log / set_poll_interval / watch_state_entry / watch_state_uninitialized / update_watch_state`　`// Go: vfswatch.go:*`
- [ ] `snapshot_paths / snapshot_dir_entry / hash_entries`（xxh3，排序 dirs/files）/ `current_state` / `has_changes` / `has_changes_from_watch_state` / `wait_for_settled` / `run`　`// Go: vfswatch.go:*`

### `vfsmock`（Go: `vfsmock/wrapper.go` + mock_generated）

- [ ] `FsMock`（每方法一个可设闭包字段，记录调用）+ `wrap(fs)`（用底层 fs 的方法填充）　`// Go: wrapper.go:Wrap` + mock_generated

### Cargo / crate 接线

- [ ] `internal/vfs/Cargo.toml`（`name = "tsgo_vfs"`，deps：tspath/core/collections + dashmap/xxhash-rust/encoding_rs）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明全部子模块 + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `Fs` trait + 内存 `MapFs`（vfstest）基础读写——tracer bullet（`TestIOFS`/`TestVFSTestMapFS` 等靠它）。
2. BOM 解码（`TestBOM`：UTF-8/UTF-16 LE/BE）。
3. 大小写敏感/不敏感 + canonical 去重（`TestInsensitive`/`TestSensitive`/`Test*DuplicatePath`）。
4. 写/删/symlink（`TestWritableFS*`/`TestSymlink*`）。
5. `cachedvfs` 缓存命中计数（`TestDirectoryExists` 等 10 个，用 mock 计调用次数）。
6. `vfsmatch` glob（**最大**）：先 `match_segments`/`match_wildcard`（`TestMatchSegmentsEdgeCases`/`TestGlobPatternInternals`）→ `compile_glob_pattern` → `SpecMatcher`（`TestSpecMatcher*`）→ `read_directory`（`TestReadDirectory`/baselines ~80 case）。
7. `vfswatch`（`TestHasChangesNoRedundantGetAccessibleEntries` + race 测试转 Rust 并发测试）。
8. `osvfs`（需真实磁盘/临时目录；`TestOS`/`TestSymlinkRealpath`/`TestGetAccessibleEntries`，部分平台特化）。

## 与 Go 的已知偏离（divergence）

- **`io/fs` 抽象缺失**：见核心偏离。`iovfs`+`internal`+`vfstest` 在内存路径上的职责合并为自写内存 FS；osvfs 直接基于 `std::fs`。
- **去 unsafe**：`internal.ReadFile` 与 `vfstest.WriteFile` 的 `unsafe.String`/`unsafe.Slice` 零拷贝改安全 `String`/`Vec`（`// PERF(port)`）。
- **`fstest.MapFS` → 自写内存树**：vfstest 基于 Go 的 `testing/fstest.MapFS`；Rust 自写 `HashMap<canonicalPath, FileNode>` + symlinks 表，复刻其符号链接跟随/大小写/realpath 语义。
- **平台特化**：realpath（darwin/linux/windows/other）+ reparse point（windows）用 `#[cfg(target_os=...)]`。Windows reparse/junction 测试归平台特定。
- **并发**：osvfs 信号量用 `tsgo_core::Semaphore`；vfswatch 的 `Mutex` + 轮询用 `std::sync::Mutex` + `std::thread`；race 测试转 Rust（`loom`/`-race` 等价）。
- **xxh3 哈希**：`zeebo/xxh3` → `xxhash-rust`（xxh3），需保证目录 children hash 的稳定性（排序后哈希，顺序对齐）。
- **`time.Time` mtime**：→ `SystemTime`；vfstest 的 `Clock` 用于确定性时间。
- **moq mock**：`vfsmock.FSMock` → `mockall` 或手写记录闭包；`TestWrap` 用反射检查所有字段非零→ Rust 用编译期保证或测试逐方法。

## 转交 / 推迟（DEFER）

- `osvfs` 平台特化（Windows reparse point、各平台 realpath）需在对应平台验证；`reparsepoint_windows_test.go` 归 Windows CI。
- vfswatch 的 race/fuzz 测试转 Rust 并发/property 测试，实现期补。
- `vfsmatch` 与 TS `matchFiles` baseline 的完整对拍归 P10（本包已含大量 baseline 用例）。
- `GetGlobalTypingsCacheLocation` 的真实使用在 project（P8）。
