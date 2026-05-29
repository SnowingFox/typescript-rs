# project: 测试清单（tests.md）

> 已实读全部 26 个 `*_test.go`（顶层 18 + `ata/` 4 + `background/` 1 + `dirty/` 1 + `logging/` 1，含 2 个 `TestMain`），逐 `func Test*`、逐表驱动/`t.Run` 子用例对齐。
> **完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
> **Go 测试规模**：26 文件 / 39 顶层 `func Test*`（含 2 `TestMain`）/ 约 190 子用例。
> 大量集成测试依赖 `testutil/projecttestutil`（mock client/typings installer/session）→ 标 `—(P10)`，但仍逐子用例列出以便届时直接 TDD。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | crate | 顶层函数 |
|---|---|---|---|
| `dirty/syncmap_test.go` | `dirty/syncmap.rs` tests | `tsgo_project_dirty` | 1 |
| `background/queue_test.go` | `background/queue.rs` tests | `tsgo_project_background` | 1 |
| `logging/logtree_test.go` | `logging/logtree.rs` tests | `tsgo_project_logging` | 2 |
| `ata/validatepackagename_test.go` | `ata/validatepackagename.rs` tests | `tsgo_project_ata` | 1 |
| `ata/discovertypings_test.go` | `ata/discovertypings.rs` tests | `tsgo_project_ata` | 1 |
| `ata/ata_test.go` | `tests/ata.rs` | `tsgo_project_ata` | 1 |
| `ata/installnpmpackages_test.go` | `tests/installnpmpackages.rs` | `tsgo_project_ata` | 1 |
| `ata/testmain_test.go` | harness | — | 1 (TestMain) |
| `overlayfs_test.go` | `overlayfs.rs` tests | `tsgo_project` | 1 |
| `watch_test.go` | `watch.rs` tests | `tsgo_project` | 2 |
| `watchtimeout_test.go` | `tests/watchtimeout.rs` | `tsgo_project` | 1 |
| `refcountcache_test.go` | `tests/refcountcache.rs` | `tsgo_project` | 1 |
| `extendedconfigcache_test.go` | `tests/extendedconfigcache.rs` | `tsgo_project` | 1 |
| `snapshot_test.go` | `tests/snapshot.rs` | `tsgo_project` | 1 |
| `snapshotfs_test.go` | `tests/snapshotfs.rs` | `tsgo_project` | 5 |
| `project_test.go` | `tests/project.rs` | `tsgo_project` | 5 |
| `projectcollection*_test.go`（2） | `tests/projectcollection*.rs` | `tsgo_project` | 2 |
| `projectlifetime_test.go` | `tests/projectlifetime.rs` | `tsgo_project` | 1 |
| `projectreferencesprogram_test.go` | `tests/projectreferencesprogram.rs` | `tsgo_project` | 1 |
| `session_test.go` | `tests/session.rs` | `tsgo_project` | 1 |
| `configfilechanges_test.go` | `tests/configfilechanges.rs` | `tsgo_project` | 1 |
| `customconfigfilename_test.go` | `tests/customconfigfilename.rs` | `tsgo_project` | 1 |
| `bulkcache_test.go` | `tests/bulkcache.rs` | `tsgo_project` | 1 |
| `untitled_test.go` | `tests/untitled.rs` | `tsgo_project` | 3 |
| `testmain_test.go` | harness | — | 1 (TestMain) |

---

## 纯单元测试（P8 内收口；不依赖 projecttestutil）

### `dirty/syncmap_test.go` → `TestSyncMapProxyFor`（4 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `syncmap_proxy_for_race_condition` | 两 goroutine 并发 Change 同一 key → 一个 entry 设 proxyFor 指向另一个，两者最终值一致、皆 dirty | `.../proxy for race condition` | |
| `syncmap_proxy_operations_delegation` | proxy 的 Change/ChangeIf/Value/Dirty/Locked 全部转发到 target | `.../proxy operations delegation` | |
| `syncmap_proxy_delete_operations` | proxy 的 Delete/DeleteIf 转发到 target，key 被删除 | `.../proxy delete operations` | |
| `syncmap_no_proxy_when_no_race` | 单 entry 修改 → proxyFor 为 nil，dirty=true，值="changed" | `.../no proxy when no race` | |

### `background/queue_test.go` → `TestQueue`（4 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `queue_basic_enqueue` | 单任务执行 | `.../BasicEnqueue` | |
| `queue_multiple_tasks_execution` | 10 任务全执行，counter==10 | `.../MultipleTasksExecution` | |
| `queue_nested_enqueue` | 任务内嵌套 enqueue，executed 长度==2 | `.../NestedEnqueue` | |
| `queue_closed_rejects_new_tasks` | Close 后 enqueue 不执行 | `.../ClosedQueueRejectsNewTasks` | |

### `logging/logtree_test.go`（2 函数）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `logtree_implements_logger` | `LogTree` 实现 `Log(...)` 接口（编译期断言） | `TestLogTreeImplementsLogger` | |
| `logtree_smoke` | 占位（Go 当前为空 body）；Rust 补：Fork/Embed/String 树形输出 + 头 `======== name ========` | `TestLogTree` | |

### `ata/validatepackagename_test.go` → `TestValidatePackageName`（11 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `validate_name_too_long` | "a"×256 → `NameTooLong` | `.../name cannot be too long` | |
| `validate_starts_with_dot` | ".foo" → `NameStartsWithDot` | `.../package name cannot start with dot` | |
| `validate_starts_with_underscore` | "_foo" → `NameStartsWithUnderscore` | `.../package name cannot start with underscore` | |
| `validate_non_uri_safe` | "  scope  ", "; say ‘...’ #", "a/b/c" → `NameContainsNonURISafeCharacters` | `.../package non URI safe characters...` | |
| `validate_scoped_ok` | "@scope/bar" → `NameOk` | `.../scoped package name is supported` | |
| `validate_scope_starts_with_dot` | "@.scope/bar","@.scope/.bar" → `NameStartsWithDot`, name=".scope", isScope=true | `.../scoped name...cannot start with dot` | |
| `validate_scope_starts_with_underscore` | "@_scope/bar","@_scope/_bar" → `NameStartsWithUnderscore`, name="_scope", isScope=true | `.../scoped name...cannot start with dot`(2) | |
| `validate_scope_non_uri_safe` | "@  scope  /bar" 等 → `NameContainsNonURISafeCharacters`, name="  scope  ", isScope=true | `.../scope name...non URI safe...` | |
| `validate_pkg_in_scope_dot` | "@scope/.bar" → `NameStartsWithDot`, name=".bar", isScope=false | `.../package name in scoped...cannot start with dot` | |
| `validate_pkg_in_scope_underscore` | "@scope/_bar" → `NameStartsWithUnderscore`, name="_bar", isScope=false | `.../package name in scoped...underscore` | |
| `validate_pkg_in_scope_non_uri_safe` | "@scope/  bar  " 等 → `NameContainsNonURISafeCharacters`, name="  bar  ", isScope=false | `.../package name in scoped...non URI safe...` | |

### `overlayfs_test.go` → `TestProcessChanges`（8 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `process_changes_multiple_opens_panic` | 同批两次 open → panic | `.../multiple opens should panic` | |
| `process_changes_watch_create_then_delete` | create+delete 抵消为 nothing | `.../watch create then delete becomes nothing` | |
| `process_changes_watch_delete_then_create` | delete+create → change | `.../watch delete then create becomes change` | |
| `process_changes_dedup_watch_changes` | 多 watch change 去重 | `.../multiple watch changes deduplicated` | |
| `process_changes_save_marks_matching_disk` | save 标 overlay matchesDiskText=true | `.../save marks overlay as matching disk` | |
| `process_changes_watch_change_marks_not_matching` | overlay 上 watch change → matchesDiskText=false | `.../watch change on overlay marks as not matching disk` | |
| `process_changes_save_without_overlay_no_panic` | 无 overlay 的 save 不 panic | `.../save without overlay should not panic` | |
| `process_changes_close_then_open_marks_changed` | 同批 close+open → Changed | `.../close then open in same batch marks as changed` | |

### `watch_test.go`（2 函数）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `get_path_components_for_watching` | `/project`→`["/","project"]`；`C:\project`→`["C:/","project"]`；`//server/share/project/tsconfig.json`→`["//server/share","project","tsconfig.json"]`；`\\server\share\...` 同；`C:\Users`→`["C:/Users"]`；`C:\Users\andrew\project`→`["C:/Users/andrew","project"]`；`/home`→`["/home"]`；`/home/andrew/project`→`["/home/andrew","project"]` | `TestGetPathComponentsForWatching` | |
| `nil_watched_files_clone` | nil `WatchedFiles.Clone(42)` → nil | `TestNilWatchedFilesClone` | |

### `refcountcache_test.go` → `TestRefCountingCaches`（嵌套：parseCache 5 + extendedConfigCache 2 子用例）

> 依赖 projecttestutil（session）→ `—(P10)`，但语义可对 `RefCountCache`/`OwnerCache` 单元化（无 session 版本）。

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `refcount_parsecache_reuse_unchanged_file` | 未变文件复用 AST（refCount 共享） | `parseCache/reuse unchanged file` | —(P10) |
| `refcount_parsecache_release_on_close` | 关闭文件释放 AST | `parseCache/release file on close` | —(P10) |
| `refcount_parsecache_unchanged_program_no_over_ref` | 未变 program 不过度 ref | `parseCache/unchanged program does not over-ref` | —(P10) |
| `refcount_parsecache_fallback_rebuild_no_double_ref` | 回退重建不重复 ref 变更文件 | `parseCache/fallback rebuild does not double-ref changed file` | —(P10) |
| `refcount_parsecache_case_only_dup_released` | 大小写重复加载在 dispose 时释放 | `parseCache/case-only duplicate loads are released on dispose` | —(P10) |
| `refcount_extended_release_with_project_close` | 项目关闭释放 extended config | `extendedConfigCache/release extended configs with project close` | —(P10) |
| `refcount_extended_release_unretained_clone` | 未保留克隆释放缓存项 | `extendedConfigCache/release cache entries for unretained clone` | —(P10) |

### `extendedconfigcache_test.go` → `TestExtendedConfigCacheOwnership`（4 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `extended_multi_extends_shared_ancestor_once` | 多 extends 共享祖先只计一次（owners 集合） | `.../multi-extends shared ancestor counted once` | —(P10) |
| `extended_same_path_twice_case_insensitive` | ExtendedSourceFiles 可含同路径两次（大小写） | `.../ExtendedSourceFiles can contain same path twice (case-insensitive)` | —(P10) |
| `extended_project_dedupes_case_only_via_cache` | 项目系统经缓存去重大小写 extends | `.../project system dedupes case-only extends via cache` | —(P10) |
| `extended_transitive_ownership_new_project` | 传递 extends 所有权 + 新项目 | `.../transitive extended config ownership with new project` | —(P10) |

---

## 集成测试（依赖 projecttestutil → 多数 `—(P10)`；逐子用例列出）

### `ata/discovertypings_test.go` → `TestDiscoverTypings`（9 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `discover_uses_safe_list_mappings` | 从 safe list 映射文件名→类型名 | `.../should use mappings from safe list` | |
| `discover_returns_node_for_core_modules` | core 模块返回 node | `.../should return node for core modules` | |
| `discover_uses_cached_locations` | 使用缓存的 typing 位置 | `.../should use cached locations` | |
| `discover_handles_removed_from_registry` | 优雅处理已从 registry 移除的包 | `.../should gracefully handle packages...removed...` | |
| `discover_searches_only_2_levels` | node_modules 仅搜 2 层深 | `.../should search only 2 levels deep` | |
| `discover_supports_scoped_packages` | 支持 scoped 包（depth 3） | `.../should support scoped packages` | |
| `discover_installs_expired_typings` | 安装过期 typings | `.../should install expired typings` | |
| `discover_installs_expired_prerelease` | prerelease tsserver 下安装过期 typings | `.../should install expired typings with prerelease version...` | |
| `discover_prerelease_typings_handled` | prerelease typings 正确处理 | `.../prerelease typings are properly handled` | |

### `ata/installnpmpackages_test.go` → `TestInstallNpmPackages`（2 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `install_npm_command_too_long_batches` | 命令过长时分批安装 | `.../works when the command is too long...` | |
| `install_npm_partial_failure_continues` | 部分命令失败仍装剩余包 | `.../installs remaining packages when one...fails` | |

### `ata/ata_test.go` → `TestATA`（~26 子用例）

| Rust 测试 | 验证内容（简） | Go 对照 | 完成 |
|---|---|---|---|
| `ata_local_module_not_picked_up` | 本地模块不被 ATA | `.../local module should not be picked up` | —(P10) |
| `ata_configured_projects` | configured 项目 ATA | `.../configured projects` | —(P10) |
| `ata_inferred_projects` | inferred 项目 ATA | `.../inferred projects` | —(P10) |
| `ata_disable_filename_based` | disableFilenameBasedTypeAcquisition:true | `.../type acquisition with disableFilenameBasedTypeAcquisition:true` | —(P10) |
| `ata_discover_from_node_modules` | 从 node_modules 发现 | `.../discover from node_modules` | —(P10) |
| `ata_discover_node_modules_empty_types` | node_modules empty types | `.../discover from node_modules empty types` | —(P10) |
| `ata_discover_node_modules_explicit_types` | node_modules explicit types | `.../discover from node_modules explicit types` | —(P10) |
| `ata_discover_node_modules_empty_types_has_import` | empty types + import | `.../discover from node_modules empty types has import` | —(P10) |
| `ata_discover_from_bower_components` | 从 bower_components 发现 | `.../discover from bower_components` | —(P10) |
| `ata_discover_from_bower_json` | 从 bower.json 发现 | `.../discover from bower.json` | —(P10) |
| `ata_malformed_package_json_watched` | 畸形 package.json 仍被监听 | `.../Malformed package.json should be watched` | —(P10) |
| `ata_redo_resolution_after_install` | 安装后重做解析（.js→typings） | `.../should redo resolution that resolved to '.js'...` | —(P10) |
| `ata_expired_cache_inferred_install` | 过期缓存（inferred）应安装 | `.../expired cache entry (inferred project, should install typings)` | —(P10) |
| `ata_non_expired_cache_inferred_no_install` | 未过期（inferred）不安装 | `.../non-expired cache entry (inferred...should not install...)` | —(P10) |
| `ata_dedup_local_types_packages` | 去重本地 @types 包 | `.../deduplicate from local @types packages` | —(P10) |
| `ata_expired_cache_lockfile3_install` | 过期缓存 lockfile3 安装 | `.../expired cache entry...lockfile3` | —(P10) |
| `ata_non_expired_lockfile3_no_install` | 未过期 lockfile3 不安装 | `.../non-expired cache entry...lockfile3` | —(P10) |
| `ata_install_for_unresolved_imports` | 为未解析 import 安装 typings | `.../should install typings for unresolved imports` | —(P10) |
| `ata_watch_disabled_no_panic` | WatchEnabled=false 不 panic | `.../ATA with WatchEnabled false should not panic` | —(P10) |
| `ata_disabled_variants` | ATA 经多种方式禁用（子表 `ATA disabled via <name>`） | `.../ATA disabled via ...` | —(P10) |
| `ata_reenabled_triggers_diag_refresh` | 重新启用触发诊断刷新 | `.../ATA re-enabled after being disabled triggers diagnostics refresh` | —(P10) |

### `snapshotfs_test.go`（5 函数，~40 子用例）

#### `TestSnapshotFSBuilder`（8 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `sfs_builder_builds_dir_tree_on_add` | 加文件建目录树 | `.../builds directory tree on file add` | —(P10) |
| `sfs_builder_builds_nested_dir_tree` | 建嵌套目录树 | `.../builds nested directory tree` | —(P10) |
| `sfs_builder_removes_dir_on_delete` | 删文件移除目录项 | `.../removes directory entries on file delete` | —(P10) |
| `sfs_builder_removes_only_empty_dirs` | 仅移除空目录 | `.../removes only empty directories on file delete` | —(P10) |
| `sfs_builder_adds_to_existing_dir` | 加文件到已有目录 | `.../adds file to existing directory` | —(P10) |
| `sfs_builder_no_change_no_add_delete` | 无增删无变化 | `.../no change when no files added or deleted` | —(P10) |
| `sfs_builder_overlay_over_disk` | overlay 优先于磁盘 | `.../overlay files are returned over disk files` | —(P10) |
| `sfs_builder_multi_add_delete_single_cycle` | 单周期多增删 | `.../multiple files added and deleted in single cycle` | —(P10) |

#### `TestSnapshotFS`（4 子用例）

| `sfs_overlay_dirs_from_overlays` | overlay 目录由 overlay 计算 | `.../overlay directories are computed from overlays` | —(P10) |
| `sfs_accessible_entries_combines` | GetAccessibleEntries 合并磁盘+overlay | `.../GetAccessibleEntries combines disk and overlay` | —(P10) |
| `sfs_getfile_overlay` | GetFile 返回 overlay | `.../GetFile returns overlay file` | —(P10) |
| `sfs_getfile_disk_when_not_overlay` | 不在 overlay 时返回磁盘文件 | `.../GetFile returns disk file when not in overlay` | —(P10) |

#### `TestSourceFS`（~9 子用例）

| `source_fs_getfile_reads_fs` | 未缓存时从 fs 读 | `.../GetFile reads from fs when not cached` | —(P10) |
| `source_fs_getfile_nil_nonexistent` | 不存在返回 nil | `.../GetFile returns nil for non-existent file` | —(P10) |
| `source_fs_is_open_file_overlays` | overlay 为 open file | `.../isOpenFile returns true for overlays` | —(P10) |
| `source_fs_getfilebypath_uses_path` | GetFileByPath 用提供路径 | `.../GetFileByPath uses provided path` | —(P10) |
| `source_fs_accessible_entries_combines` | 合并磁盘+overlay 目录 | `.../GetAccessibleEntries combines disk and overlay directories` | —(P10) |
| `source_fs_tracks_when_enabled` | 启用跟踪时记 seenFiles | `.../tracks files when tracking enabled` | —(P10) |
| `source_fs_no_track_when_disabled` | 禁用时不跟踪 | `.../does not track files when tracking disabled` | —(P10) |
| `source_fs_disable_tracking_stops` | DisableTracking 停止跟踪 | `.../DisableTracking stops tracking` | —(P10) |
| `source_fs_file_exists` / `source_fs_read_file` | FileExists/ReadFile 透传 source | `.../FileExists returns true...` / `.../ReadFile returns content...` | —(P10) |

#### `TestAutoImportBuilderFS`（1 子用例）

| `auto_import_builder_fs_symlink_cache_mismatch` | 符号链接缓存不匹配（删除后 realpath miss） | `.../symlink cache mismatch...` | —(P10) |

#### `TestRealpathAliasLifecycle`（~13 子用例）

| `realpath_alias_recorded_on_read` | 读 symlinked node_modules 文件记别名 | `.../alias recorded when reading symlinked node_modules file` | —(P10) |
| `realpath_no_alias_outside_node_modules` | node_modules 外不记别名 | `.../no alias recorded for files outside node_modules` | —(P10) |
| `realpath_aliases_carried_across_snapshots` | 别名跨快照保留 | `.../aliases carried over across snapshots` | —(P10) |
| `realpath_alias_pruned_on_delete` | symlinked 文件删除时剪除别名 | `.../alias pruned when symlinked file is deleted` | —(P10) |
| `realpath_multiple_symlinks_same_realpath` | 多 symlink 指向同 realpath | `.../multiple symlinks to same realpath` | —(P10) |
| `realpath_multiple_symlinks_pruned_individually` | 多 symlink 分别剪除 | `.../multiple symlinks pruned individually` | —(P10) |
| `realpath_expand_change_events` | expandRealpathAliases 展开 change 事件 | `.../expandRealpathAliases expands change events` | —(P10) |
| `realpath_expand_delete_events` | 展开 delete 事件 | `.../expandRealpathAliases expands delete events` | —(P10) |
| `realpath_expand_noop_no_aliases` | 无别名时 no-op | `.../expandRealpathAliases is a no-op with no aliases` | —(P10) |
| `realpath_markdirty_via_realpath_event` | realpath 事件标脏 symlinked 文件 | `.../markDirtyFiles invalidates symlinked file via realpath event` | —(P10) |
| `realpath_alias_clone_isolation` | 别名克隆隔离（快照间） | `.../alias clone isolation between snapshots` | —(P10) |
| `realpath_add_symlink_no_mutate_prev` | 加 symlink 到继承 realpath key 不改前快照 | `.../adding symlink to inherited realpath key does not mutate previous snapshot` | —(P10) |

### `snapshot_test.go` → `TestSnapshot`（4 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `snapshot_compilerhost_frozen_once` | compilerHost 仅冻结一次（用快照 FS） | `.../compilerHost gets frozen with snapshot's FS only once` | —(P10) |
| `snapshot_cached_disk_files_cleaned` | 缓存磁盘文件被清理 | `.../cached disk files are cleaned up` | —(P10) |
| `snapshot_getfile_nil_nonexistent` | 不存在文件 GetFile 返回 nil | `.../GetFile returns nil for non-existent files` | —(P10) |
| `snapshot_program_change_loads_node_modules_autoimport` | program 变更加载 node_modules 依赖且 auto-imports 含之 | `.../program change loads node_modules dependency and auto-imports includes it` | —(P10) |

### `project_test.go`（5 函数）

#### `TestProjectProgramUpdateKind`（6 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `program_update_newfiles_on_initial` | 初次构建 → NewFiles | `.../NewFiles on initial build` | —(P10) |
| `program_update_cloned_single_file_change` | 单文件改 → Cloned | `.../Cloned on single-file change` | —(P10) |
| `program_update_samefilenames_config_no_root` | config 改无 root 变 → SameFileNames | `.../SameFileNames on config change without root changes` | —(P10) |
| `program_update_newfiles_on_root_addition` | 加 root → NewFiles | `.../NewFiles on root addition` | —(P10) |
| `program_update_samefilenames_unresolvable_import` | 多文件改加不可解析 import → SameFileNames | `.../SameFileNames when adding an unresolvable import...` | —(P10) |
| `program_update_cmdline_typings_reset` | CommandLine 改时 commandLineWithTypingsFiles 重置 | `.../commandLineWithTypingsFiles is reset on CommandLine change` | —(P10) |

#### `TestProject`（1）/`TestPushDiagnostics`（6）/`TestDisplayName`（3）/`TestProgressNotifications`（3）

| `project_smoke` | 基本项目行为 | `TestProject` | —(P10) |
| `push_diag_initial_program` | 初次创建发布程序诊断 | `TestPushDiagnostics/publishes program diagnostics on initial program creation` | —(P10) |
| `push_diag_clears_on_remove` | 项目移除时清诊断 | `.../clears diagnostics when project is removed` | —(P10) |
| `push_diag_updates_on_program_change` | program 变更更新诊断 | `.../updates diagnostics when program changes` | —(P10) |
| `push_diag_not_for_inferred` | inferred 项目不发布 | `.../does not publish for inferred projects` | —(P10) |
| `push_diag_global_after_checking` | 检查后发布全局诊断 | `.../publishes global diagnostics after checking` | —(P10) |
| `push_diag_clean_restore_on_close_reopen` | TS 文件关闭清 tsconfig 诊断、重开恢复 | `.../cleans tsconfig diagnostics after TS files close...` | —(P10) |
| `display_name_configured_relative` | configured 返回相对 config 路径 | `TestDisplayName/configured project returns relative config path` | —(P10) |
| `display_name_configured_nested` | 嵌套 config | `.../configured project with nested config` | —(P10) |
| `display_name_inferred_dir_base` | inferred 返回目录基名 | `.../inferred project returns directory base name` | —(P10) |
| `progress_emits_configured` | configured 加载发进度 | `TestProgressNotifications/emits progress for configured project loading` | —(P10) |
| `progress_emits_inferred` | inferred 加载发进度 | `.../emits progress for inferred project loading` | —(P10) |
| `progress_start_matches_finish` | 每 start 有匹配 finish | `.../each start has a matching finish` | —(P10) |

### `projectcollectiondefaultproject_test.go` → `TestProjectCollectionDefaultProject`

| `pc_default_project` | 默认项目选择（含 solution/引用搜索）；逐 t.Run 子表 | `TestProjectCollectionDefaultProject` | —(P10) |

### `projectcollectionbuilder_test.go` → `TestProjectCollectionBuilder`（13 子用例）

| Rust 测试 | 验证内容（简） | Go 对照 | 完成 |
|---|---|---|---|
| `pcb_solution_direct_ref` | solution 直接引用默认项目 | `.../when project found is solution referencing default project directly` | —(P10) |
| `pcb_solution_indirect_ref` | 间接引用 | `.../...indirectly` | —(P10) |
| `pcb_solution_disable_ref_load_direct` | disableReferencedProjectLoad 直接 | `.../...disableReferencedProjectLoad referencing...directly` | —(P10) |
| `pcb_solution_indirect_via_disable_ref_load` | 经 disableReferencedProjectLoad 间接 | `.../...indirectly through disableReferencedProjectLoad` | —(P10) |
| `pcb_indirect_disable_one_not_another` | 一处禁用一处不禁用 | `.../...in one but without it in another` | —(P10) |
| `pcb_own_files_ref_from_referenced` | 自有文件引用被引用项目的文件 | `.../when project found is project with own files...` | —(P10) |
| `pcb_ancestor_folder_lookup` | 文件不在首个 config 树→查祖先文件夹及其引用 | `.../when file is not part of first config tree...` | —(P10) |
| `pcb_dts_next_to_ts_root_in_referenced` | dts 与 ts 相邻且作 root | `.../when dts file is next to ts file...` | —(P10) |
| `pcb_issue_1630` | 回归 #1630 | `.../#1630` | —(P10) |
| `pcb_inferred_root_stable_order` | inferred root 文件稳定顺序 | `.../inferred project root files are in stable order` | —(P10) |
| `pcb_lookup_terminates` | 项目查找终止（无死循环） | `.../project lookup terminates` | —(P10) |
| `pcb_file_moves_to_inferred_after_import_deleted` | 删 import 后文件移到 inferred | `.../file moves to inferred project after import is deleted` | —(P10) |
| `pcb_update_on_package_json_change` | package.json 改更新项目 | `.../should update project on package.json change` | —(P10) |

### `projectlifetime_test.go` → `TestProjectLifetime`（6 子用例）

| `lifetime_configured_project` | configured 项目生命周期 | `.../configured project` | —(P10) |
| `lifetime_unrooted_inferred` | 无根 inferred 项目 | `.../unrooted inferred projects` | —(P10) |
| `lifetime_move_inferred_to_configured` | 文件从 inferred 移到 configured | `.../file moves from inferred to configured project` | —(P10) |
| `lifetime_move_via_open_close` | 经 didOpen/didClose 移动 | `.../file move from inferred to configured via didOpen/didClose sequence` | —(P10) |
| `lifetime_tsconfig_move_subdir_to_parent` | tsconfig 经 watch 从子目录移到父 | `.../tsconfig move from subdirectory to parent...` | —(P10) |
| `lifetime_deleted_open_file_remains_until_closed` | 删除的打开文件保留至关闭 | `.../deleted open file remains in project until closed` | —(P10) |

### `projectreferencesprogram_test.go` → `TestProjectReferencesProgram`（11 子用例）

| `prp_program_for_referenced` | 被引用项目的 program | `.../program for referenced project` | —(P10) |
| `prp_disable_source_redirect` | disableSourceOfProjectReferenceRedirect | `.../program with disableSourceOfProjectReferenceRedirect` | —(P10) |
| `prp_symlink_index_typings` | 经 symlink 引用（index+typings） | `.../references through symlink with index and typings` | —(P10) |
| `prp_symlink_preserve_symlinks` | preserveSymlinks | `.../...with preserveSymlinks` | —(P10) |
| `prp_symlink_scoped` | scoped 包 | `.../...scoped package` | —(P10) |
| `prp_symlink_scoped_preserve` | scoped + preserveSymlinks | `.../...scoped package preserveSymlinks` | —(P10) |
| `prp_symlink_from_subfolder` | 从子文件夹引用 | `.../references through symlink referencing from subFolder` | —(P10) |
| `prp_symlink_from_subfolder_preserve` | + preserveSymlinks | `.../...from subFolder with preserveSymlinks` | —(P10) |
| `prp_symlink_from_subfolder_scoped` | + scoped | `.../...from subFolder scoped package` | —(P10) |
| `prp_symlink_from_subfolder_scoped_preserve` | + scoped + preserveSymlinks | `.../...scoped package preserveSymlinks` | —(P10) |
| `prp_new_file_added_to_referenced` | 被引用项目新增文件 | `.../when new file is added to referenced project` | —(P10) |

### `session_test.go` → `TestSession`（嵌套，~45 子用例）

> 顶层 `TestSession` 下按事件分组。每个最内层 `t.Run` 一条 Rust `#[test]`。

| Rust 测试（命名按路径） | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `session_didopen_create_configured` | 开文件建 configured 项目 | `DidOpenFile/create configured project` | —(P10) |
| `session_didopen_create_inferred` | 建 inferred 项目 | `DidOpenFile/create inferred project` | —(P10) |
| `session_didopen_inferred_in_memory` | 内存文件 inferred | `DidOpenFile/inferred project for in-memory files` | —(P10) |
| `session_didopen_inferred_js` | JS 文件 inferred | `DidOpenFile/inferred project JS file` | —(P10) |
| `session_watchchange_didopen_same_batch_rebuilds` | 同批 watchChange+didOpen 重建 program | `watchChange and didOpen in same batch rebuilds program` | —(P10) |
| `session_didchange_update_file_program` | 改文件更新 program | `DidChangeFile/update file and program` | —(P10) |
| `session_didchange_update_untitled` | 改 untitled 文件 | `DidChangeFile/update untitled file` | —(P10) |
| `session_didchange_reuses_unchanged` | 未变 source file 复用 | `DidChangeFile/unchanged source files are reused` | —(P10) |
| `session_didchange_pulls_new_files` | 改动引入新文件 | `DidChangeFile/change can pull in new files` | —(P10) |
| `session_didchange_then_config_reloads` | 单文件改后 config 改重载 program | `DidChangeFile/single-file change followed by config change reloads program` | —(P10) |
| `session_didclose_configured_delete_close_recreate` | configured：删/关/重建 | `DidCloseFile/Configured projects/delete a file, close it, recreate it` | —(P10) |
| `session_didclose_inferred_delete_close_recreate` | inferred：删/关/重建 | `DidCloseFile/Inferred projects/delete a file, close it, recreate it` | —(P10) |
| `session_didclose_close_untitled` | 关 untitled 文件 | `DidCloseFile/Inferred projects/close untitled file` | —(P10) |
| `session_didsave_save_event_first` | save 事件在先 | `DidSaveFile/save event first` | —(P10) |
| `session_didsave_watch_event_first` | watch 事件在先 | `DidSaveFile/watch event first` | —(P10) |
| `session_source_sharing_similar_options` | 相似选项项目共享 source files | `Source file sharing/projects with similar options share source files` | —(P10) |
| `session_source_sharing_different_options` | 不同选项不共享 | `Source file sharing/projects with different options do not share source files` | —(P10) |
| `session_watched_change_open_file` | 改打开文件 | `DidChangeWatchedFiles/change open file` | —(P10) |
| `session_watched_change_closed_program_file` | 改已关闭的程序文件 | `.../change closed program file` | —(P10) |
| `session_watched_change_program_file_not_in_root` | 改不在 tsconfig root 的程序文件（子表 workspaceDir=*） | `.../change program file not in tsconfig root files` | —(P10) |
| `session_watched_change_config_file` | 改 config 文件 | `.../change config file` | —(P10) |
| `session_watched_delete_explicit_file` | 删显式包含文件 | `.../delete explicitly included file` | —(P10) |
| `session_watched_delete_wildcard_file` | 删通配包含文件 | `.../delete wildcard included file` | —(P10) |
| `session_watched_delete_dir_wildcard` | 删含通配文件的目录 | `.../delete directory with wildcard included files` | —(P10) |
| `session_watched_delete_dir_program_only` | 删含 program-only 文件目录 | `.../delete directory with program-only files` | —(P10) |
| `session_watched_delete_sibling_schedules_refresh` | 删兄弟文件夹调度诊断刷新 | `.../delete sibling folder schedules diagnostics refresh` | —(P10) |
| `session_watched_delete_sibling_refresh_after_third` | 开第三文件后删兄弟调度刷新 | `.../...after opening third file` | —(P10) |
| `session_watched_create_explicit_file` | 创建显式包含文件 | `.../create explicitly included file` | —(P10) |
| `session_watched_create_failed_lookup` | 创建失败查找位置 | `.../create failed lookup location` | —(P10) |
| `session_watched_create_wildcard_file` | 创建通配包含文件 | `.../create wildcard included file` | —(P10) |
| `session_watched_irrelevant_ext_filtered` | 无关扩展名变更被过滤 | `.../irrelevant extension changes are filtered out` | —(P10) |
| `session_watched_pnpm_install_links_local` | pnpm install 链接本地包 | `.../pnpm install links local package` | —(P10) |
| `session_watched_symlinked_pkgjson_invalidates` | symlinked node_modules package.json 改使解析失效 | `.../symlinked node_modules package.json change...` | —(P10) |
| `session_watched_create_in_nonexistent_dir` | 在不存在目录创建文件 | `.../create file in non-existent directory` | —(P10) |
| `session_watched_create_symlink_dir_matching` | 创建匹配 include 的 symlink 目录 | `.../create symlink directory matching include pattern` | —(P10) |
| `session_refreshes_codelens_inlayhints_on_pref` | 相关偏好变更刷新 code lens/inlay hints | `refreshes code lenses and inlay hints when relevant user preferences change` | —(P10) |
| `session_config_parsing` | config 解析 | `config parsing` | —(P10) |
| `session_ls_closed_file_in_configured_not_opened` | configured 中未打开的关闭文件 LS | `language service for closed files/closed file in configured project not yet opened` | —(P10) |
| `session_ls_closed_file_no_config_creates_inferred` | 无 config 的关闭文件创建 inferred | `.../closed file with no configured project creates inferred project` | —(P10) |
| `session_jsconfig_for_js_when_tsconfig_same_dir` | 同目录有 tsconfig 时 JS 用 jsconfig | `jsconfig.json used for JS files when tsconfig.json exists in same directory` | —(P10) |

### `configfilechanges_test.go` → `TestConfigFileChanges`（3 子用例）

| `cfc_update_program_options_on_config_change` | config 改更新 program 选项 | `.../should update program options on config file change` | —(P10) |
| `cfc_update_on_extended_config_change` | extended config 改更新项目 | `.../should update project on extended config file change` | —(P10) |
| `cfc_update_on_doubly_extended_change` | 双层 extended config 改 | `.../should update project on doubly extended config file change` | —(P10) |

### `customconfigfilename_test.go` → `TestCustomConfigFileName`（8 子用例）

| `ccf_picks_up_custom_switches_on_pref` | 拾取自定义 config 并随偏好切换 | `.../picks up custom config and switches on preference change` | —(P10) |
| `ccf_uses_tsconfig_when_empty` | 空时用 tsconfig.json | `.../uses tsconfig.json when customConfigFileName is empty` | —(P10) |
| `ccf_falls_back_when_missing` | 自定义缺失回退 tsconfig.json | `.../falls back to tsconfig.json when custom config missing` | —(P10) |
| `ccf_reverts_when_cleared` | 清除偏好恢复 tsconfig.json | `.../reverts to tsconfig.json when custom config preference is cleared` | —(P10) |
| `ccf_schedules_diag_refresh_on_change` | 偏好变更调度诊断刷新 | `.../schedules diagnostics refresh when custom config preference changes` | —(P10) |
| `ccf_rejects_path_traversal` | 拒绝路径穿越 | `.../rejects path traversal in customConfigFileName` | —(P10) |
| `ccf_accepts_plain_base_names` | 接受纯基名 | `.../accepts plain base file names in customConfigFileName` | —(P10) |
| `ccf_cleans_inferred_when_custom_covers` | 自定义 config 覆盖文件时清 inferred | `.../cleans up inferred project when custom config covers file` | —(P10) |

### `bulkcache_test.go` → `TestBulkCacheInvalidation`

| `bulk_cache_invalidation` | 批量缓存失效（excessive watch events） | `TestBulkCacheInvalidation`（含子表） | —(P10) |

### `untitled_test.go`（3 函数）

| `untitled_references` | untitled 文件引用查找 | `TestUntitledReferences` | —(P10) |
| `untitled_in_inferred_project` | untitled 在 inferred 项目 | `TestUntitledFileInInferredProject` | —(P10) |
| `untitled_imports` | untitled 文件中的 import | `TestImportsInUntitled` | —(P10) |

### `watchtimeout_test.go` → `TestUpdateWatchTimeoutAndRollback`（1 子用例，synctest 虚拟时钟）

| `watch_timeout_retry_same_identity` | 超时回滚后下次更新用相同 watcher ID 重试并成功（虚拟时钟推进 2s 过 1s 超时） | `.../watch retries on next snapshot update after timeout with same watcher identity` | —(P10) |

---

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（39 个，含 2 TestMain）都已映射
- [x] 每个 `t.Run` 子用例（含嵌套 session_test）都已逐行列出
- [x] expected 值取自 Go 测试字面量（validatepackagename 的状态/name/isScope、watch 的路径分量、queue 的计数等）
- [x] 每条带 `// Go:` 锚点（Go 对照列）
- [x] 与 impl.md 双向对齐：dirty/background/logging/ata/缓存族/FS/项目/会话的实现 TODO 均承载对应测试

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 全部依赖 `projecttestutil` 的集成测试（session/project/lifetime/pcbuilder/refs/snapshot/snapshotfs/ata/configfilechanges/customconfig/bulkcache/untitled/extendedconfig/refcountcache/watchtimeout） | mock client/typings installer/session init 设施在 P10 | P10 |
| `xxh3` 缓存 key 字节级一致性向量 | 需确定 Rust xxh3 实现 | 执行期（P8 内验证） |
| `runtime/metrics` telemetry | Rust 无直接等价 | DEFER |
| 可在 P8 内收口的纯单元：dirty/syncmap、background/queue、logging/logtree、ata/validatepackagename、overlayfs/processChanges、watch 路径函数 | 不依赖 projecttestutil | P8 |
