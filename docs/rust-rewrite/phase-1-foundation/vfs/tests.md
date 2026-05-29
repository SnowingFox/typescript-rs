# vfs: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：13 测试文件 / ~58 顶层 `func Test`（+ 3 `Fuzz` + 若干 `Benchmark`）/ 约 200+ 子用例（其中 `vfsmatch` 占大头）。

> 按子包分节。表驱动（`cases := []...{...}` + `t.Run`）逐子用例列；命令式序列按断言列关键点。Race/Fuzz/Benchmark 标推迟。`testutil.AssertPanics`/`gotest.tools/assert` → Rust `#[should_panic]`/`catch_unwind`/断言。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `iovfs/iofs_test.go` | `iovfs/mod.rs` | 1（8 子测试） |
| `cachedvfs/cachedvfs_test.go` | `cachedvfs/mod.rs` | 10 |
| `vfstest/vfstest_test.go` | `vfstest/mod.rs` | 18 |
| `vfswatch/vfswatch_test.go` | `vfswatch/mod.rs` | 1 |
| `vfswatch/vfswatch_race_test.go` | `vfswatch/mod.rs` | 5（+2 Fuzz，推迟） |
| `vfsmatch/vfsmatch_test.go` | `vfsmatch/mod.rs` | 17 |
| `vfsmock/wrapper_test.go` | `vfsmock/mod.rs` | 1 |
| `osvfs/os_test.go` | `osvfs/mod.rs` | 1（3 子测试） |
| `osvfs/realpath_test.go` | `osvfs/mod.rs` | 2 |
| `osvfs/reparsepoint_windows_test.go` | `osvfs/mod.rs`（`#[cfg(windows)]`） | 平台特定，推迟 |
| `vfs/vfs_test.go` | （Benchmark only） | 0 |

## `iovfs/iofs_test.go` — `TestIOFS`（8 子测试）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `iofs_read_file` | `read_file("/foo.ts")`→Some("hello, world")；`/does/not/exist.ts`→(None, "") | `iofs_test.go:TestIOFS/ReadFile` | |
| `iofs_read_file_unrooted_panics` | `read_file("bar")` panic `vfs: path "bar" is not absolute` | `.../ReadFileUnrooted` | |
| `iofs_file_exists` | `/foo.ts`→true、`/bar`→false | `.../FileExists` | |
| `iofs_directory_exists` | `/`/`/dir1`/`/dir1/`/`/dir1/./`→true、`/bar`→false | `.../DirectoryExists` | |
| `iofs_get_accessible_entries` | `/` → dirs `[dir1,dir2]`、files `[foo.ts]` | `.../GetAccessibleEntries` | |
| `iofs_walk_dir` | walk `/` 收集文件（排序）→ `[/dir1/file1.ts,/dir1/file2.ts,/dir2/file1.ts,/foo.ts]` | `.../WalkDir` | |
| `iofs_walk_dir_skip` | walk 返回 SkipDir → 仅 `[/foo.ts]` | `.../WalkDirSkip` | |
| `iofs_realpath` | `realpath("/foo.ts")`→`/foo.ts` | `.../Realpath` | |
| `iofs_use_case_sensitive` | `use_case_sensitive_file_names()`→true | `.../UseCaseSensitiveFileNames` | |

## `cachedvfs/cachedvfs_test.go`（10 个缓存计数测试）

> 模式：调底层 mock，断言底层调用次数；缓存命中不增；ClearCache/DisableAndClearCache/Enable 切换行为。底层 mock = `vfsmock.Wrap(vfstest.FromMap({"/some/path/file.txt":"hello world"}, true))`。

| Rust 测试 | 验证内容（调用次数序列） | Go 对照 | 完成 |
|---|---|---|---|
| `cached_directory_exists` | 同 path 2 次→底层 1；ClearCache 后→2；新 path→3；DisableAndClearCache 后 2 次→4,5；Enable 后 2 次→6,6（缓存恢复） | `cachedvfs_test.go:TestDirectoryExists` | |
| `cached_file_exists` | 同上序列 1,1,2,3,4,5,6,6 | `cachedvfs_test.go:TestFileExists` | |
| `cached_get_accessible_entries` | 同上 1,1,2,3,4,5,6,6 | `cachedvfs_test.go:TestGetAccessibleEntries` | |
| `cached_realpath` | 同上 1,1,2,3,4,5,6,6 | `cachedvfs_test.go:TestRealpath` | |
| `cached_stat` | 同上 1,1,2,3,4,5,6,6 | `cachedvfs_test.go:TestStat` | |
| `cached_read_file_not_cached` | **不缓存**：每次都增 1,2,3,4,5,6,7 | `cachedvfs_test.go:TestReadFile` | |
| `cached_use_case_sensitive_not_cached` | 不缓存：1,2,3,4,5,6,7 | `cachedvfs_test.go:TestUseCaseSensitiveFileNames` | |
| `cached_walk_dir_not_cached` | 不缓存：1..7 | `cachedvfs_test.go:TestWalkDir` | |
| `cached_remove_not_cached` | 不缓存：1..7 | `cachedvfs_test.go:TestRemove` | |
| `cached_write_file_not_cached` | 不缓存：1,2,3,4,5,6,7；并验证第 3 次 call.Path/Data 透传正确 | `cachedvfs_test.go:TestWriteFile` | |

## `vfstest/vfstest_test.go`（18 测试）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `vfstest_insensitive` | 大小写不敏感读/stat/realpath/readdir | `foo/bar/baz` 与 `Foo/Bar/Baz` 同内容；realpath 还原小写；不存在→"file does not exist" | `vfstest_test.go:TestInsensitive` | |
| `vfstest_insensitive_upper` | 大写键的不敏感访问 | `Foo/Bar/Baz` 键，`foo/bar/baz` 也能读；readdir 返回原大写名 | `.../TestInsensitiveUpper` | |
| `vfstest_sensitive` | 敏感：仅精确大小写命中 | `foo/bar/baz`✓；`Foo/Bar/Baz`→"file does not exist" | `.../TestSensitive` | |
| `vfstest_sensitive_duplicate_path_panics` | 敏感下重复 canonical panic | `foo`+`Foo` → panic `duplicate path: "Foo" and "foo" have the same canonical path` | `.../TestSensitiveDuplicatePath` | |
| `vfstest_insensitive_duplicate_path_ok` | 不敏感下重复键不 panic | `foo`+`Foo`（不敏感）→ 不 panic | `.../TestInsensitiveDuplicatePath` | |
| `vfstest_writable` | 写/读/覆盖/写入非目录报错 | write `/foo/bar/baz`→读回；覆盖；write `/foo/bar/baz/oops`→err `mkdir "foo/bar/baz": path exists but is not a directory` | `.../TestWritableFS` | |
| `vfstest_writable_delete` | 删文件/目录/不存在不报错/前缀边界 | 删 file、删 dir 递归、删不存在→nil；`/foo/barbar` 不被 `/foo/bar` 删 | `.../TestWritableFSDelete` | |
| `vfstest_stress` | 并发随机操作不崩 | GOMAXPROCS goroutine × 10000 op | `.../TestStress`（→ Rust 并发测试） | |
| `vfstest_parent_dir_file_panics` | 父路径是文件 panic | `foo`(file)+`foo/oops` → panic `failed to create intermediate directories for "foo/oops": mkdir "foo": path exists but is not a directory` | `.../TestParentDirFile` | |
| `vfstest_from_map_posix` | POSIX 路径 string/bytes/MapFile | `/string`/`/bytes`/`/mapfile` 都读出 "hello, world" | `.../TestFromMap/POSIX` | |
| `vfstest_from_map_windows` | Windows 盘符路径 | `c:/string`/`d:/bytes`/`e:/mapfile` 读出 | `.../TestFromMap/Windows` | |
| `vfstest_from_map_mixed_panics` | 混合 posix/windows panic | `/string`+`c:/bytes` → panic `mixed posix and windows paths` | `.../TestFromMap/Mixed` | |
| `vfstest_from_map_nonrooted_panics` | 非 rooted panic | `string` → panic `non-rooted path "string"` | `.../TestFromMap/NonRooted` | |
| `vfstest_from_map_nonnormalized_panics` | 非规范化 panic | `/string/` → panic `non-normalized path "/string/"` | `.../TestFromMap/NonNormalized` | |
| `vfstest_from_map_nonnormalized2_panics` | `..` 段 panic | `/string/../foo` → panic `non-normalized path "/string/../foo"` | `.../TestFromMap/NonNormalized2` | |
| `vfstest_from_map_invalid_file_panics` | 非法文件类型 panic | `/string`: 1234 → panic `invalid file type int` | `.../TestFromMap/InvalidFile` | |
| `vfstest_mapfs_read_realpath_case` | 内存 FS 读 + realpath 大小写 | `/foo.ts`✓；`/Foo.ts` realpath→`/foo.ts`；不存在 realpath 原样；不敏感 | `.../TestVFSTestMapFS`（ReadFile/Realpath/UseCaseSensitiveFileNames） | |
| `vfstest_mapfs_windows` | Windows 盘符内存 FS | `c:/foo.ts`✓；`c:/Foo.ts` realpath→`c:/foo.ts` | `.../TestVFSTestMapFSWindows` | |
| `vfstest_bom_be` | UTF-16 BE BOM 解码 | `FE FF`+UTF16 → "hello, world" | `.../TestBOM/BigEndian` | |
| `vfstest_bom_le` | UTF-16 LE BOM 解码 | `FF FE`+UTF16 → "hello, world" | `.../TestBOM/LittleEndian` | |
| `vfstest_bom_utf8` | UTF-8 BOM 剥离 | `EF BB BF`+text → "hello, world" | `.../TestBOM/UTF8` | |
| `vfstest_symlink` | 符号链接读/realpath/exists | `/symlink.ts`→foo 内容；dir symlink；链式 a→b→c→d；realpath 还原 | `.../TestSymlink`（ReadFile/Realpath/FileExists/DirectoryExists） | |
| `vfstest_writable_symlink` | 经 symlink 写、broken symlink 行为 | 经 dirlink 写；写 dirlink 本身报错；broken dir symlink 写报错；broken file symlink 可写 | `.../TestWritableFSSymlink` | |
| `vfstest_writable_symlink_chain` | 链式 symlink 下写 | 经 `/a`(→b→c→d) 写 `/a/foo/bar/new.ts`，`/b`/`/d` 同见 | `.../TestWritableFSSymlinkChain` | |
| `vfstest_writable_symlink_chain_not_dir` | 链尾是文件报错 | `/d`=文件 → 写 `/a/foo/bar/new.ts` err `mkdir "d": path exists but is not a directory` | `.../TestWritableFSSymlinkChainNotDir` | |
| `vfstest_writable_symlink_delete` | 删 symlink 不删目标；目标删后 symlink 仍在 | 删 `/a`（symlink）→ `/b`/`/c`/`/d` 仍在；删 `/d` → 链失效；写回恢复；broken link 写恢复 | `.../TestWritableFSSymlinkDelete` | |

## `vfswatch/vfswatch_test.go`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `vfswatch_no_redundant_get_accessible_entries` | 仅对 wildcard 树目录调 GetAccessibleEntries，各一次 | wildcard `/src`(recursive)，explicit 含 `/node_modules` → HasChangesFromWatchState 后 GetAccessibleEntries 调用 **2** 次（`/src`、`/src/sub`），`/node_modules` 只 Stat | `vfswatch_test.go:TestHasChangesNoRedundantGetAccessibleEntries` | |

## `vfswatch/vfswatch_race_test.go`（并发，转 Rust 并发测试 / `-race`）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `vfswatch_race_haschanges_vs_update` | HasChanges 读 vs UpdateWatchState 写无数据竞争 | `vfswatch_race_test.go:TestRaceHasChangesVsUpdateWatchState` | — (实现期) |
| `vfswatch_race_wildcard_dirs` | wildcardDirectories 并发读写无竞争 | `.../TestRaceWildcardDirectoriesAccess` | — |
| `vfswatch_race_poll_interval` | pollInterval 并发读写无竞争 | `.../TestRacePollIntervalAccess` | — |
| `vfswatch_race_mixed` | 全操作并发无竞争 | `.../TestRaceMixedOperations` | — |
| `vfswatch_race_update_with_fs_mods` | FS 改动 vs UpdateWatchState 扫描无竞争 | `.../TestRaceUpdateWithConcurrentFileModifications` | — |

## `vfsmatch/vfsmatch_test.go`（17 测试；glob 匹配，最大）

### `TestReadDirectory`（~70 named cases）

> input：host + extensions + includes + excludes + depth → expected（精确列表或包含/不含断言）。Go 对照统一 `vfsmatch_test.go:TestReadDirectory/<name>`。

| Rust 测试（case 名） | 关键 input → expected | 完成 |
|---|---|---|
| `defaults_include_common_package_folders` | commonFolders, ext[ts,tsx,d.ts], 无 includes → 含 a.ts/b.ts/x/a.ts/node_modules/a.ts/bower_components/a.ts/jspm_packages/a.ts | |
| `literal_includes_without_exclusions` | includes[a.ts,b.ts] → `[/dev/a.ts,/dev/b.ts]` | |
| `literal_includes_non_ts_excluded` | includes[a.js,b.js] → 空 | |
| `literal_includes_missing_excluded` | includes[z.ts,x.ts] → 空 | |
| `literal_includes_with_literal_excludes` | excl[b.ts] incl[a.ts,b.ts] → `[/dev/a.ts]` | |
| `literal_includes_with_wildcard_excludes` | excl[*.ts,z/??z.ts,*/b.ts] incl[a.ts,b.ts,z/a.ts,z/abz.ts,z/aba.ts,x/b.ts] → `[/dev/z/a.ts,/dev/z/aba.ts]` | |
| `literal_includes_with_recursive_excludes` | excl[**/b.ts] incl[a.ts,b.ts,x/a.ts,x/b.ts,x/y/a.ts,x/y/b.ts] → `[/dev/a.ts,/dev/x/a.ts,/dev/x/y/a.ts]` | |
| `case_sensitive_exclude_respected` | caseSensitive, excl[**/b.ts] incl[B.ts] → `[/dev/B.ts]` | |
| `explicit_includes_keep_common_package_folders` | incl 显式含 node_modules/a.ts 等 → 含全部 | |
| `wildcard_include_sorted_order` | incl[z/*.ts,x/*.ts] → 固定排序列表（z 段 6 个 + x 段 3 个） | |
| `wildcard_include_same_named_declarations_excluded` | incl[*.ts] → 含 a.ts/b.ts/a.d.ts/c.d.ts | |
| `wildcard_star_matches_only_ts` | incl[*] → 全是 .ts/.tsx/.d.ts，不含 .js | |
| `wildcard_question_single_char` | incl[x/?.ts] → `[/dev/x/a.ts,/dev/x/b.ts]` | |
| `wildcard_recursive_directory` | incl[**/a.ts] → 含 dev/a.ts、z/a.ts、x/a.ts、x/y/a.ts | |
| `double_asterisk_zero_or_more_dirs` | incl[x/**/a.ts] → len 2，含 x/a.ts、x/y/a.ts | |
| `wildcard_multiple_recursive_dirs` | incl[x/y/**/a.ts,x/**/a.ts,z/**/a.ts] → len>0 | |
| `wildcard_case_sensitive_matching` | caseSensitive incl[**/A.ts] → `[/dev/A.ts]` | |
| `wildcard_missing_files_excluded` | incl[*/z.ts] → 空 | |
| `exclude_folders_with_wildcards` | excl[z,x] incl[**/*] → 不含 /z/ /x/，含 a.ts/b.ts | |
| `include_paths_outside_project_absolute` | incl[*,/ext/*] → 含 /dev/a.ts、/ext/ext.ts | |
| `include_paths_outside_project_relative` | excl[**] incl[*,../ext/*] → 含 /ext/ext.ts | |
| `include_files_double_dots` | excl[**] incl[/ext/b/a..b.ts] → 含 /ext/b/a..b.ts | |
| `exclude_files_double_dots` | excl[/ext/b/a..b.ts] incl[/ext/**/*] → 含 ext.ts、不含 a..b.ts | |
| `common_package_folders_implicitly_excluded` | incl[**/a.ts] → 含 dev/a.ts、x/a.ts；不含 node_modules/bower/jspm | |
| `common_package_folders_explicit_recursive` | incl[**/a.ts,**/node_modules/a.ts] → 含 node_modules/a.ts | |
| `common_package_folders_wildcard` | incl[*/a.ts] → 含 x/a.ts、不含 node_modules/a.ts | |
| `common_package_folders_explicit_wildcard` | incl[*/a.ts,node_modules/a.ts] → 含两者 | |
| `dotted_folders_not_implicitly_included` | dottedFolders incl[x/**/*,w/*/*] → 含 x/d.ts、x/y/d.ts；不含 .y/.e/.u | |
| `dotted_folders_explicitly_included` | incl[x/.y/a.ts,/dev/.z/.b.ts] → 含两者 | |
| `dotted_folders_recursive_wildcard` | incl[**/.*/*] → 含 x/.y/a.ts、.z/c.ts、w/.u/e.ts | |
| `trailing_recursive_include_empty` | incl[**] → 空 | |
| `trailing_recursive_exclude_removes_all` | excl[**] incl[**/*] → 空 | |
| `multiple_recursive_dir_in_includes` | incl[**/x/**/*] → 含 x/a.ts、x/y/a.ts | |
| `multiple_recursive_dir_in_excludes` | excl[**/x/**] incl[**/a.ts] → 含 a.ts、z/a.ts；不含 x/a.ts、x/y/a.ts | |
| `implicit_globbification_expands_directory` | incl[z] → 含 z/a.ts、z/aba.ts、z/b.ts | |
| `exclude_patterns_starting_starstar` | caseSensitive excl[**/x] → 不含 /x/ | |
| `include_patterns_starting_starstar` | caseSensitive incl[**/x,**/a/**/b] → 含 x/a.ts、q/a/c/b/d.ts | |
| `depth_limit_one` | depth 1 → 仅顶层（无嵌套 /） | |
| `depth_limit_two` | depth 2 → 含 dev/a.ts、z/a.ts；不含 x/y/a.ts | |
| `mixed_extensions_only_ts` | mixedExt ext[.ts] → 全 .ts | |
| `mixed_extensions_ts_tsx` | ext[.ts,.tsx] → 全 .ts/.tsx | |
| `mixed_extensions_js_jsx` | ext[.js,.jsx] → 全 .js/.jsx | |
| `min_js_excluded_by_wildcard` | ext[.js] incl[js/*] → 含 a.js/b.js；不含 d.min.js/ab.min.js | |
| `min_js_exclusion_case_sensitive` | caseSensitive ext[.js] incl[js/*] → 含 d.MIN.js（仅小写 .min.js 默认排） | |
| `min_js_explicitly_included` | incl[js/*.min.js] → 含 d.min.js、ab.min.js | |
| `min_js_included_when_pattern_mentions_min` | incl[js/*.min.*] → len 2 含两 min.js | |
| `exclude_literal_node_modules` | excl[node_modules] incl[**/*] → 含 a.ts、不含 node_modules/a.ts | |
| `same_named_declarations_include_ts` | sameNamed incl[*.ts] → len>0 | |
| `same_named_declarations_include_tsx` | incl[*.tsx] → 全 .tsx | |
| `empty_includes_returns_all_matching` | 无 includes → len>0 含 a.ts | |
| `nil_extensions_returns_all` | nil extensions → 含 a.ts、a.js | |
| `empty_extensions_slice_returns_all` | extensions=[] → len>0 | |

### `TestIsImplicitGlob`（10 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `is_implicit_glob/simple` | `"foo"`→true | `vfsmatch_test.go:TestIsImplicitGlob/simple` | |
| `.../folder` | `"src"`→true | `.../folder` | |
| `.../with_extension` | `"foo.ts"`→false | `.../with extension` | |
| `.../trailing_dot` | `"foo."`→false | `.../trailing dot` | |
| `.../star` | `"*"`→false | `.../star` | |
| `.../question` | `"?"`→false | `.../question` | |
| `.../star_suffix` | `"foo*"`→false | `.../star suffix` | |
| `.../question_suffix` | `"foo?"`→false | `.../question suffix` | |
| `.../dot_name` | `"foo.bar"`→false | `.../dot name` | |
| `.../empty` | `""`→true | `.../empty` | |

### `TestReadDirectoryEdgeCases`（8 cases）

| Rust 测试 | 关键 input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `edge_rooted_include_path` | incl[/dev/a.ts] → 含 a.ts | `...EdgeCases/rooted include path` | |
| `edge_include_with_extension` | incl[a.ts] → 含 a.ts | `.../include with extension in path` | |
| `edge_special_regex_chars` | host 含 `file+test.ts` 等，incl[file+test.ts] → 含 file+test.ts | `.../special regex characters in path` | |
| `edge_include_question_prefix` | incl[?.ts] → 含 a.ts、b.ts | `.../include pattern starting with question mark` | |
| `edge_include_star_prefix` | incl[*b.ts] → 含 b.ts | `.../include pattern starting with star` | |
| `edge_case_insensitive_matching` | host File.ts/FILE.ts(敏感建表但 useCase=true) incl[*.ts] → len 2 | `.../case insensitive file matching` | |
| `edge_nested_subdir_base_path` | caseSensitive incl[q/a/c/b/d.ts] → 含 q/a/c/b/d.ts | `.../nested subdirectory base path` | |
| `edge_current_dir_differs` | incl[z/*.ts] → len>0 | `.../current directory differs from path` | |

### `TestReadDirectoryEmptyIncludes` / `TestReadDirectorySymlinkCycle`

| Rust 测试 | 关键 input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `empty_includes_slice_behavior` | path `/root` incl[] → 空或含 /root/a.ts | `...TestReadDirectoryEmptyIncludes/empty includes slice behavior` | |
| `symlink_cycle_detected` | `/root/a/b`→symlink `/root/a`，incl[**/*] → `[/root/file.ts,/root/a/file.ts]`（环被跳过） | `...TestReadDirectorySymlinkCycle/detects and skips symlink cycles` | |

### `TestReadDirectoryMatchesTypeScriptBaselines`（~25 cases，对拍 TS baseline）

> 关键 baseline，逐 case；Go 对照 `vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/<name>`。

| Rust 测试（case 名） | 关键 input → expected | 完成 |
|---|---|---|
| `baseline_sorted_include_order_then_alpha` | incl[z/*.ts,x/*.ts] → 固定排序（z 6 + x 3） | |
| `baseline_recursive_wildcards_match_dotted_dirs` | incl[**/.*/*] → 4 项（.z/c.ts、g.min.js/.g/g.ts、w/.u/e.ts、x/.y/a.ts） | |
| `baseline_common_package_folders_implicitly_excluded_wildcard` | incl[**/a.ts] → `[/dev/a.ts,/dev/x/a.ts]` | |
| `baseline_js_wildcard_excludes_min_js` | ext[.js] incl[js/*] → `[/dev/js/a.js,/dev/js/b.js]` | |
| `baseline_explicit_min_js_pattern` | incl[js/*.min.js] → 含 ab.min.js、d.min.js（len 2） | |
| `baseline_literal_excludes` | excl[b.ts] incl[a.ts,b.ts] → `[/dev/a.ts]` | |
| `baseline_wildcard_excludes` | excl[*.ts,z/??z.ts,*/b.ts] incl[...] → `[/dev/z/a.ts,/dev/z/aba.ts]` | |
| `baseline_recursive_excludes` | excl[**/b.ts] incl[...] → `[/dev/a.ts,/dev/x/a.ts,/dev/x/y/a.ts]` | |
| `baseline_question_mark` | incl[x/?.ts] → `[/dev/x/a.ts,/dev/x/b.ts]` | |
| `baseline_recursive_directory_pattern` | incl[**/a.ts] → `[/dev/a.ts,/dev/x/a.ts,/dev/x/y/a.ts,/dev/z/a.ts]` | |
| `baseline_case_sensitive` | caseSensitive incl[**/A.ts] → `[/dev/A.ts]` | |
| `baseline_exclude_folders` | excl[z,x] incl[**/*] → 不含 /z/ /x/，含 a.ts/b.ts | |
| `baseline_implicit_glob_expansion` | incl[z] → z 段 6 个排序 | |
| `baseline_trailing_recursive_directory` | incl[**] → 空 | |
| `baseline_exclude_trailing_recursive` | excl[**] incl[**/*] → 空 | |
| `baseline_multiple_recursive_dir_patterns` | incl[**/x/**/*] → 含 x/a.ts、x/aa.ts、x/b.ts、x/y/a.ts、x/y/b.ts | |
| `baseline_include_dirs_starstar_prefix` | caseSensitive incl[**/x,**/a/**/b] → 含 x/a.ts、x/b.ts、q/a/c/b/d.ts | |
| `baseline_dotted_folders_not_implicit` | dottedFolders incl[x/**/*,w/*/*] → 含 x/d.ts、x/y/d.ts；不含 .y/.e/.u | |
| `baseline_include_paths_outside_project` | incl[*,/ext/*] → 含 dev/a.ts、ext/ext.ts | |
| `baseline_include_files_double_dots` | excl[**] incl[/ext/b/a..b.ts] → 含 a..b.ts | |
| `baseline_exclude_files_double_dots` | excl[/ext/b/a..b.ts] incl[/ext/**/*] → 含 ext.ts、不含 a..b.ts | |

### `TestSpecMatcher` 系列（5 个测试）

| Rust 测试 | 关键 input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `spec_matcher_simple_wildcard` | `*.ts` Files → match `/project/a.ts`；not `/project/a.js`、`/project/sub/a.ts` | `vfsmatch_test.go:TestSpecMatcher/simple wildcard` | |
| `spec_matcher_recursive_wildcard` | `**/*.ts` → match sub/deep；not .js | `.../recursive wildcard` | |
| `spec_matcher_exclude_pattern` | `node_modules` Exclude → match `/project/node_modules/foo`；not `/project/node_modules`、`/project/src` | `.../exclude pattern` | |
| `spec_matcher_case_insensitive` | `*.ts` 不敏感 → match `A.TS`/`B.Ts` | `.../case insensitive` | |
| `spec_matcher_multiple_specs` | `*.ts,*.tsx` → match a.ts、b.tsx | `.../multiple specs` | |
| `spec_matcher_match_string_simple` | paths→[true,false,false] | `...TestSpecMatcher_MatchString/simple wildcard files` | |
| `spec_matcher_match_string_recursive` | →[true,true,false] | `.../recursive wildcard files` | |
| `spec_matcher_match_string_exclude_prefix` | →[false,true,false] | `.../exclude pattern matches prefix` | |
| `single_spec_match_string_wildcard` | →[true,false,false] | `...TestSingleSpecMatcher_MatchString/single spec wildcard` | |
| `single_spec_match_string_trailing_starstar_exclude` | `**` Exclude →[true,true] | `.../single spec trailing starstar exclude allowed` | |
| `spec_matchers_match_index_first_match` | `*.ts,*.tsx` →[0,1,-1] | `...TestSpecMatchers_MatchIndex/index lookup prefers first match` | |
| `spec_matchers_match_index_exclude` | `node_modules,bower_components` →[-1,0,-1,1,-1] | `.../exclude index lookup` | |
| `single_spec_matcher_simple` | `*.ts` → match a.ts、not a.js | `...TestSingleSpecMatcher/simple spec` | |
| `single_spec_matcher_trailing_starstar_nonexclude_nil` | `**` Files → matcher nil | `.../trailing ** non-exclude returns nil` | |
| `single_spec_matcher_trailing_starstar_exclude_works` | `**` Exclude → match anything | `.../trailing ** exclude works` | |
| `spec_matchers_multiple_index` | `*.ts,*.tsx,*.js` → a.ts:0、b.tsx:1、c.js:2、d.css:-1 | `...TestSpecMatchers/multiple specs return correct index` | |
| `spec_matchers_empty_nil` | specs[] → nil | `.../empty specs returns nil` | |

### `TestGlobPatternInternals`（内部，多子测试）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `glob_next_path_part_consecutive_slashes` | `/dev//foo///bar` 逐段：""(off1)→dev→foo→bar | `...TestGlobPatternInternals/nextPathPart handles consecutive slashes` | |
| `glob_next_path_part_trailing_slashes` | `/dev/` 尾部斜杠→not ok | `.../path ending with slashes` | |
| `glob_next_path_part_empty_prefix` | prefix "" suffix `/dev//foo` → ""/dev/foo | `.../empty prefix` | |
| `glob_next_path_part_only_slashes_remain` | prefix `/dev/` suffix `foo` → root/dev/foo/notok | `.../only slashes remain` | |
| `glob_next_path_part_from_suffix` | prefix `/` suffix `a` → ""/a | `.../parses from suffix region` | |
| `glob_question_mark_at_end` | `a?` matches `/ab`、not `/a` | `.../question mark segment at end of string` | |
| `glob_star_complex_pattern` | `a*b*c` matches abc/aXbYc/aXXXbYYYc、not aXbY | `.../star segment with complex pattern` | |
| `glob_ensure_trailing_slash_existing` | `/dev/`→`/dev/`、`/`→`/` | `.../ensureTrailingSlash with existing slash` | |
| `glob_ensure_trailing_slash_empty` | ``→`` | `.../ensureTrailingSlash with empty string` | |
| `glob_literal_with_package_folder_include` | 显式 literal `node_modules/pkg/index.ts` → 含之 | `.../literal component with package folder in include` | |

### `TestMatchSegmentsEdgeCases`（多子测试）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `match_seg_question_before_slash` | `a?b` matches aXb、not ab/aXYb | `...TestMatchSegmentsEdgeCases/question mark before slash in string` | |
| `match_seg_star_no_trailing` | `a*` matches a/abc/aXYZ | `.../star with no trailing content` | |
| `match_seg_multiple_stars` | `*a*` matches a/Xa/aX/XaY、not XYZ | `.../multiple stars in pattern` | |
| `match_seg_backtracking` | `*a*a`/`*a*b*c`/`*a*a*a`/`a*b*a` 多组（含负例） | `.../multiple stars requiring backtracking` | |
| `match_seg_pathological_perf` | `*a*a*a*a*b` 对 16 个 a 快速返回 false | `.../pathological pattern performance` | |
| `match_seg_literal_not_matching` | `abcdefgh.ts` not abc.ts、match exact | `.../literal segment not matching` | |
| `match_seg_question_multibyte` | `?.ts` matches a/é/中/🎉.ts、not .ts/ab.ts；`??.ts` 两 rune | `.../question mark matches multi-byte unicode rune` | |
| `match_seg_star_multibyte` | `*é.ts` matches café.ts not cafe.ts；`*🎉*` | `.../star matches multi-byte unicode runes correctly` | |

### 其余 vfsmatch 测试

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `readdir_consecutive_slashes` | `**/*.ts` 找到 a.ts、x/b.ts | `vfsmatch_test.go:TestReadDirectoryConsecutiveSlashes` | |
| `glob_literal_pkg_wildcard_skips` | `*/*.ts` 跳过 node_modules/b.ts | `...TestGlobPatternLiteralWithPackageFolders/wildcard skips package folders` | |
| `glob_literal_pkg_explicit_includes` | `node_modules/b.ts` 包含 | `.../explicit literal includes package folder` | |
| `get_base_paths_case_sensitive_no_dedup` | caseSensitive `../Other/**/*.ts`,`../other/**/*.ts` → 含 /Other 与 /other | `...TestGetBasePathsCaseSensitivity/case-sensitive does not dedup differently-cased paths` | |
| `get_base_paths_case_insensitive_dedup` | 不敏感 → /Other 与 /other 至多一个 | `.../case-insensitive dedups differently-cased paths` | |

## `vfsmock/wrapper_test.go`

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `vfsmock_wrap_all_fields_set` | `Wrap` 后所有导出字段非零（每方法都接好） | `vfsmock/wrapper_test.go:TestWrap` | |

## `osvfs/os_test.go` + `realpath_test.go`（需真实磁盘 / 临时目录）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `os_read_file` | 读仓库 `go.mod`/`Cargo.toml` 与 std 读一致 | `os_test.go:TestOS/ReadFile` | |
| `os_realpath` | home 目录 realpath（Windows 盘符大写） | `.../Realpath` | |
| `os_use_case_sensitive` | Windows→false、Linux→true | `.../UseCaseSensitiveFileNames` | |
| `os_symlink_realpath` | target 与 link 的 realpath 相等 | `realpath_test.go:TestSymlinkRealpath` | |
| `os_get_accessible_entries_symlinks` | 含 symlink 目录 → Symlinks 集 4 项；非 symlink 目录 → Symlinks 空集 | `realpath_test.go:TestGetAccessibleEntries` | |

## 0 直接单测的子包（补充行为级测试）

- `trackingvfs` / `wrapvfs` / `internal` **无直接单测**，行为由 P10 兜底。补行为级：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `trackingvfs_records_reads` | 读类记录路径、写类不记 | read_file/file_exists/stat 后 seen_files 含路径；write/remove 不加 | trackingvfs.go:* | |
| `wrapvfs_replacement_used` | 有 replacement 用之、否则委派 | 设 FileExists replacement → 用之；未设 → 委派底层 | wrapvfs.go:* | |
| `internal_split_path` | 拆根 | `/c:/foo` → root/rest；非绝对 panic | internal.go:SplitPath/RootLength | |

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（cachedvfs 10 + iofs 1 + vfstest 18 + vfswatch 1+5 + vfsmatch 17 + vfsmock 1 + osvfs 1+2）
- [x] 表驱动子用例逐行列出（vfsmatch TestReadDirectory ~70 + baselines ~25 + SpecMatcher 系列 + IsImplicitGlob 等）
- [x] 命令式序列按断言关键点列出（cachedvfs 调用计数、vfstest symlink）
- [x] expected 取自 Go 测试字面量（路径列表、panic 消息、调用次数）
- [x] 每条带 `// Go:` 锚点
- [x] Race/Fuzz/平台特定/Benchmark 标推迟
- [x] 与 impl.md 双向对齐：被测子包在 impl.md 均有 TODO

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| vfswatch race（5 个）+ 2 Fuzz | 转 Rust 并发/property 测试 | 实现期 |
| `reparsepoint_windows_test.go`：`TestIsReparsePoint`（8 子用例：regular file/dir、junction、file/dir symlink、nonexistent、empty、null-byte）、`TestIsReparsePointLongPath`、`TestIsReparsePointNestedInSymlink`、`TestIsReparsePointRelativePath` | Windows 平台特定（`#[cfg(windows)]`） | Windows CI / 实现期 |
| osvfs 平台 realpath 各分支 | 各 OS 验证 | 实现期 + 各平台 CI |
| `vfsmatch` 与 TS `matchFiles` 全 baseline 对拍 | 需 submodule 语料 | P10 parity |
| `TestStress`（并发压测） | 转 Rust 并发测试 | 实现期 |
