# tspath: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：4 文件 / 24 顶层 `func Test`（另有 3 个 `Fuzz*` + 4 个 `Benchmark*`）/ 约 280+ 断言子用例。

> 本包测试分两类：(a) **命令式 `assert.Equal` 序列**（`path_test.go` 多数）——按断言序逐行列；(b) **表驱动 `t.Run`**（`ignoredpaths`/`startsWithDirectory`/`PathIsRelative`/`UntitledPathEdgeCases`/`GetCommonParents`）——逐子用例列。expected 全部取自 Go 字面量。Fuzz（对拍 `*_old` 旧实现）转 Rust property test，见推迟表。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/tspath/path_test.go` | `internal/tspath/path.rs`（`#[cfg(test)] mod tests`） | 17 |
| `internal/tspath/ignoredpaths_test.go` | `internal/tspath/ignoredpaths.rs` | 3 |
| `internal/tspath/startsWithDirectory_test.go` | `internal/tspath/path.rs`（StartsWithDirectory） | 2 |
| `internal/tspath/untitled_test.go` | `internal/tspath/path.rs`（untitled） | 2 |

## `path_test.go`

### `TestNormalizeSlashes`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `normalize_slashes/1` | `"a"` → `"a"` | `path_test.go:TestNormalizeSlashes` | |
| `normalize_slashes/2` | `"a/b"` → `"a/b"` | 同上 | |
| `normalize_slashes/3` | `"a\\b"` → `"a/b"` | 同上 | |
| `normalize_slashes/4` | `"\\\\server\\path"` → `"//server/path"` | 同上 | |

### `TestGetRootLength`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `get_root_length/a` | `"a"` → 0 | `path_test.go:TestGetRootLength` | |
| `.../slash` | `"/"` → 1 | 同上 | |
| `.../slash_path` | `"/path"` → 1 | 同上 | |
| `.../c_colon` | `"c:"` → 2 | 同上 | |
| `.../c_colon_d` | `"c:d"` → 0 | 同上 | |
| `.../c_colon_slash` | `"c:/"` → 3 | 同上 | |
| `.../c_colon_bslash` | `"c:\\"` → 3 | 同上 | |
| `.../unc_server` | `"//server"` → 8 | 同上 | |
| `.../unc_server_share` | `"//server/share"` → 9 | 同上 | |
| `.../unc_bslash_server` | `"\\\\server"` → 8 | 同上 | |
| `.../unc_bslash_share` | `"\\\\server\\share"` → 9 | 同上 | |
| `.../file_3slash` | `"file:///"` → 8 | 同上 | |
| `.../file_3slash_path` | `"file:///path"` → 8 | 同上 | |
| `.../file_3slash_c` | `"file:///c:"` → 10 | 同上 | |
| `.../file_3slash_cd` | `"file:///c:d"` → 8 | 同上 | |
| `.../file_3slash_c_path` | `"file:///c:/path"` → 11 | 同上 | |
| `.../file_3slash_c3a` | `"file:///c%3a"` → 12 | 同上 | |
| `.../file_3slash_c3ad` | `"file:///c%3ad"` → 8 | 同上 | |
| `.../file_3slash_c3a_path` | `"file:///c%3a/path"` → 13 | 同上 | |
| `.../file_3slash_c3A` | `"file:///c%3A"` → 12 | 同上 | |
| `.../file_3slash_c3Ad` | `"file:///c%3Ad"` → 8 | 同上 | |
| `.../file_3slash_c3A_path` | `"file:///c%3A/path"` → 13 | 同上 | |
| `.../file_localhost` | `"file://localhost"` → 16 | 同上 | |
| `.../file_localhost_slash` | `"file://localhost/"` → 17 | 同上 | |
| `.../file_localhost_path` | `"file://localhost/path"` → 17 | 同上 | |
| `.../file_localhost_c` | `"file://localhost/c:"` → 19 | 同上 | |
| `.../file_localhost_cd` | `"file://localhost/c:d"` → 17 | 同上 | |
| `.../file_localhost_c_path` | `"file://localhost/c:/path"` → 20 | 同上 | |
| `.../file_localhost_c3a` | `"file://localhost/c%3a"` → 21 | 同上 | |
| `.../file_localhost_c3ad` | `"file://localhost/c%3ad"` → 17 | 同上 | |
| `.../file_localhost_c3a_path` | `"file://localhost/c%3a/path"` → 22 | 同上 | |
| `.../file_localhost_c3A` | `"file://localhost/c%3A"` → 21 | 同上 | |
| `.../file_localhost_c3Ad` | `"file://localhost/c%3Ad"` → 17 | 同上 | |
| `.../file_localhost_c3A_path` | `"file://localhost/c%3A/path"` → 22 | 同上 | |
| `.../file_server` | `"file://server"` → 13 | 同上 | |
| `.../file_server_slash` | `"file://server/"` → 14 | 同上 | |
| `.../file_server_path` | `"file://server/path"` → 14 | 同上 | |
| `.../file_server_c` | `"file://server/c:"` → 14 | 同上 | |
| `.../file_server_cd` | `"file://server/c:d"` → 14 | 同上 | |
| `.../file_server_c_d` | `"file://server/c:/d"` → 14 | 同上 | |
| `.../file_server_c3a` | `"file://server/c%3a"` → 14 | 同上 | |
| `.../file_server_c3ad` | `"file://server/c%3ad"` → 14 | 同上 | |
| `.../file_server_c3a_d` | `"file://server/c%3a/d"` → 14 | 同上 | |
| `.../file_server_c3A` | `"file://server/c%3A"` → 14 | 同上 | |
| `.../file_server_c3Ad` | `"file://server/c%3Ad"` → 14 | 同上 | |
| `.../file_server_c3A_d` | `"file://server/c%3A/d"` → 14 | 同上 | |
| `.../http_server` | `"http://server"` → 13 | 同上 | |
| `.../http_server_path` | `"http://server/path"` → 14 | 同上 | |

### `TestPathIsAbsolute`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `path_is_absolute/posix` | `"/path/to/file.ext"` → true | `path_test.go:TestPathIsAbsolute` | |
| `.../dos` | `"c:/path/to/file.ext"` → true | 同上 | |
| `.../url` | `"file:///path/to/file.ext"` → true | 同上 | |
| `.../rel` | `"path/to/file.ext"` → false | 同上 | |
| `.../dot_rel` | `"./path/to/file.ext"` → false | 同上 | |

### `TestIsUrl`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `is_url/a` | `"a"` → false | `path_test.go:TestIsUrl` | |
| `.../slash` | `"/"` → false | 同上 | |
| `.../c` | `"c:"` → false | 同上 | |
| `.../cd` | `"c:d"` → false | 同上 | |
| `.../c_slash` | `"c:/"` → false | 同上 | |
| `.../c_bslash` | `"c:\\"` → false | 同上 | |
| `.../unc` | `"//server"` → false | 同上 | |
| `.../unc_share` | `"//server/share"` → false | 同上 | |
| `.../unc_b` | `"\\\\server"` → false | 同上 | |
| `.../unc_b_share` | `"\\\\server\\share"` → false | 同上 | |
| `.../file_path` | `"file:///path"` → true | 同上 | |
| `.../file_c` | `"file:///c:"` → true | 同上 | |
| `.../file_cd` | `"file:///c:d"` → true | 同上 | |
| `.../file_c_path` | `"file:///c:/path"` → true | 同上 | |
| `.../file_server` | `"file://server"` → true | 同上 | |
| `.../file_server_path` | `"file://server/path"` → true | 同上 | |
| `.../http_server` | `"http://server"` → true | 同上 | |
| `.../http_server_path` | `"http://server/path"` → true | 同上 | |

### `TestIsRootedDiskPath`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `is_rooted_disk_path/a` | `"a"` → false | `path_test.go:TestIsRootedDiskPath` | |
| `.../slash` | `"/"` → true | 同上 | |
| `.../c` | `"c:"` → true | 同上 | |
| `.../cd` | `"c:d"` → false | 同上 | |
| `.../c_slash` | `"c:/"` → true | 同上 | |
| `.../c_bslash` | `"c:\\"` → true | 同上 | |
| `.../unc` | `"//server"` → true | 同上 | |
| `.../unc_share` | `"//server/share"` → true | 同上 | |
| `.../unc_b` | `"\\\\server"` → true | 同上 | |
| `.../unc_b_share` | `"\\\\server\\share"` → true | 同上 | |
| `.../file_path` | `"file:///path"` → false | 同上 | |
| `.../file_c` | `"file:///c:"` → false | 同上 | |
| `.../file_cd` | `"file:///c:d"` → false | 同上 | |
| `.../file_c_path` | `"file:///c:/path"` → false | 同上 | |
| `.../file_server` | `"file://server"` → false | 同上 | |
| `.../file_server_path` | `"file://server/path"` → false | 同上 | |
| `.../http_server` | `"http://server"` → false | 同上 | |
| `.../http_server_path` | `"http://server/path"` → false | 同上 | |

### `TestGetDirectoryPath`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `get_directory_path/empty` | `""` → `""` | `path_test.go:TestGetDirectoryPath` | |
| `.../a` | `"a"` → `""` | 同上 | |
| `.../a_b` | `"a/b"` → `"a"` | 同上 | |
| `.../slash` | `"/"` → `"/"` | 同上 | |
| `.../slash_a` | `"/a"` → `"/"` | 同上 | |
| `.../slash_a_slash` | `"/a/"` → `"/"` | 同上 | |
| `.../slash_a_b` | `"/a/b"` → `"/a"` | 同上 | |
| `.../slash_a_b_slash` | `"/a/b/"` → `"/a"` | 同上 | |
| `.../c` | `"c:"` → `"c:"` | 同上 | |
| `.../cd` | `"c:d"` → `""` | 同上 | |
| `.../c_slash` | `"c:/"` → `"c:/"` | 同上 | |
| `.../c_path` | `"c:/path"` → `"c:/"` | 同上 | |
| `.../c_path_slash` | `"c:/path/"` → `"c:/"` | 同上 | |
| `.../unc` | `"//server"` → `"//server"` | 同上 | |
| `.../unc_slash` | `"//server/"` → `"//server/"` | 同上 | |
| `.../unc_share` | `"//server/share"` → `"//server/"` | 同上 | |
| `.../unc_share_slash` | `"//server/share/"` → `"//server/"` | 同上 | |
| `.../unc_b` | `"\\\\server"` → `"//server"` | 同上 | |
| `.../unc_b_slash` | `"\\\\server\\"` → `"//server/"` | 同上 | |
| `.../unc_b_share` | `"\\\\server\\share"` → `"//server/"` | 同上 | |
| `.../unc_b_share_slash` | `"\\\\server\\share\\"` → `"//server/"` | 同上 | |
| `.../file_3slash` | `"file:///"` → `"file:///"` | 同上 | |
| `.../file_path` | `"file:///path"` → `"file:///"` | 同上 | |
| `.../file_path_slash` | `"file:///path/"` → `"file:///"` | 同上 | |
| `.../file_c` | `"file:///c:"` → `"file:///c:"` | 同上 | |
| `.../file_cd` | `"file:///c:d"` → `"file:///"` | 同上 | |
| `.../file_c_slash` | `"file:///c:/"` → `"file:///c:/"` | 同上 | |
| `.../file_c_path` | `"file:///c:/path"` → `"file:///c:/"` | 同上 | |
| `.../file_c_path_slash` | `"file:///c:/path/"` → `"file:///c:/"` | 同上 | |
| `.../file_server` | `"file://server"` → `"file://server"` | 同上 | |
| `.../file_server_slash` | `"file://server/"` → `"file://server/"` | 同上 | |
| `.../file_server_path` | `"file://server/path"` → `"file://server/"` | 同上 | |
| `.../file_server_path_slash` | `"file://server/path/"` → `"file://server/"` | 同上 | |
| `.../http_server` | `"http://server"` → `"http://server"` | 同上 | |
| `.../http_server_slash` | `"http://server/"` → `"http://server/"` | 同上 | |
| `.../http_server_path` | `"http://server/path"` → `"http://server/"` | 同上 | |
| `.../http_server_path_slash` | `"http://server/path/"` → `"http://server/"` | 同上 | |

### `TestGetPathComponents`

| Rust 测试 | input(path, "") → expected | Go 对照 | 完成 |
|---|---|---|---|
| `get_path_components/empty` | `""` → `[""]` | `path_test.go:TestGetPathComponents` | |
| `.../a` | `"a"` → `["","a"]` | 同上 | |
| `.../dot_a` | `"./a"` → `["",".","a"]` | 同上 | |
| `.../slash` | `"/"` → `["/"]` | 同上 | |
| `.../slash_a` | `"/a"` → `["/","a"]` | 同上 | |
| `.../slash_a_slash` | `"/a/"` → `["/","a"]` | 同上 | |
| `.../c` | `"c:"` → `["c:"]` | 同上 | |
| `.../cd` | `"c:d"` → `["","c:d"]` | 同上 | |
| `.../c_slash` | `"c:/"` → `["c:/"]` | 同上 | |
| `.../c_path` | `"c:/path"` → `["c:/","path"]` | 同上 | |
| `.../unc` | `"//server"` → `["//server"]` | 同上 | |
| `.../unc_slash` | `"//server/"` → `["//server/"]` | 同上 | |
| `.../unc_share` | `"//server/share"` → `["//server/","share"]` | 同上 | |
| `.../file_3slash` | `"file:///"` → `["file:///"]` | 同上 | |
| `.../file_path` | `"file:///path"` → `["file:///","path"]` | 同上 | |
| `.../file_c` | `"file:///c:"` → `["file:///c:"]` | 同上 | |
| `.../file_cd` | `"file:///c:d"` → `["file:///","c:d"]` | 同上 | |
| `.../file_c_slash` | `"file:///c:/"` → `["file:///c:/"]` | 同上 | |
| `.../file_c_path` | `"file:///c:/path"` → `["file:///c:/","path"]` | 同上 | |
| `.../file_server` | `"file://server"` → `["file://server"]` | 同上 | |
| `.../file_server_slash` | `"file://server/"` → `["file://server/"]` | 同上 | |
| `.../file_server_path` | `"file://server/path"` → `["file://server/","path"]` | 同上 | |
| `.../http_server` | `"http://server"` → `["http://server"]` | 同上 | |
| `.../http_server_slash` | `"http://server/"` → `["http://server/"]` | 同上 | |
| `.../http_server_path` | `"http://server/path"` → `["http://server/","path"]` | 同上 | |

### `TestReducePathComponents`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `reduce/empty` | `[""]` → `[""]` | `path_test.go:TestReducePathComponents` | |
| `.../dot` | `["","."]` → `[""]` | 同上 | |
| `.../dot_a` | `["",".","a"]` → `["","a"]` | 同上 | |
| `.../a_dot` | `["","a","."]` → `["","a"]` | 同上 | |
| `.../dotdot` | `["",".."]` → `["",".."]` | 同上 | |
| `.../dotdot_dotdot` | `["","..",".."]` → `["","..",".."]` | 同上 | |
| `.../dotdot_dot_dotdot` | `["","..",".",".."]` → `["","..",".."]` | 同上 | |
| `.../a_dotdot` | `["","a",".."]` → `[""]` | 同上 | |
| `.../dotdot_a` | `["","..","a"]` → `["","..","a"]` | 同上 | |
| `.../root` | `["/"]` → `["/"]` | 同上 | |
| `.../root_dot` | `["/","."]` → `["/"]` | 同上 | |
| `.../root_dotdot` | `["/",".."]` → `["/"]` | 同上 | |
| `.../root_a_dotdot` | `["/","a",".."]` → `["/"]` | 同上 | |

### `TestCombinePaths`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `combine/rel` | `("path","to","file.ext")` → `"path/to/file.ext"` | `path_test.go:TestCombinePaths` | |
| `.../rel_dotdot` | `("path","dir","..","to","file.ext")` → `"path/dir/../to/file.ext"` | 同上 | |
| `.../posix` | `("/path","to","file.ext")` → `"/path/to/file.ext"` | 同上 | |
| `.../posix_abs2` | `("/path","/to","file.ext")` → `"/to/file.ext"` | 同上 | |
| `.../dos` | `("c:/path","to","file.ext")` → `"c:/path/to/file.ext"` | 同上 | |
| `.../dos_abs2` | `("c:/path","c:/to","file.ext")` → `"c:/to/file.ext"` | 同上 | |
| `.../url` | `("file:///path","to","file.ext")` → `"file:///path/to/file.ext"` | 同上 | |
| `.../url_abs2` | `("file:///path","file:///to","file.ext")` → `"file:///to/file.ext"` | 同上 | |
| `.../root_nm` | `("/","/node_modules/@types")` → `"/node_modules/@types"` | 同上 | |
| `.../a_dotdot_empty` | `("/a/..","")` → `"/a/.."` | 同上 | |
| `.../a_dotdot_b` | `("/a/..","b")` → `"/a/../b"` | 同上 | |
| `.../a_dotdot_b_slash` | `("/a/..","b/")` → `"/a/../b/"` | 同上 | |
| `.../a_dotdot_slash` | `("/a/..","/")` → `"/"` | 同上 | |
| `.../a_dotdot_slashb` | `("/a/..","/b")` → `"/b"` | 同上 | |

### `TestResolvePath`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `resolve/empty` | `("")` → `""` | `path_test.go:TestResolvePath` | |
| `.../dot` | `(".")` → `""` | 同上 | |
| `.../dot_slash` | `("./")` → `""` | 同上 | |
| `.../dotdot` | `("..")` → `".."` | 同上 | |
| `.../dotdot_slash` | `("../")` → `"../"` | 同上 | |
| `.../slash` | `("/")` → `"/"` | 同上 | |
| `.../slash_dot` | `("/.")` → `"/"` | 同上 | |
| `.../slash_dot_slash` | `("/./")` → `"/"` | 同上 | |
| `.../slash_dotdot_slash` | `("/../")` → `"/"` | 同上 | |
| `.../slash_a` | `("/a")` → `"/a"` | 同上 | |
| `.../slash_a_slash` | `("/a/")` → `"/a/"` | 同上 | |
| `.../slash_a_dot` | `("/a/.")` → `"/a"` | 同上 | |
| `.../slash_a_dot_slash` | `("/a/./")` → `"/a/"` | 同上 | |
| `.../slash_a_dot_b` | `("/a/./b")` → `"/a/b"` | 同上 | |
| `.../slash_a_dot_b_slash` | `("/a/./b/")` → `"/a/b/"` | 同上 | |
| `.../slash_a_dotdot` | `("/a/..")` → `"/"` | 同上 | |
| `.../slash_a_dotdot_slash` | `("/a/../")` → `"/"` | 同上 | |
| `.../slash_a_dotdot_b` | `("/a/../b")` → `"/b"` | 同上 | |
| `.../slash_a_dotdot_b_slash` | `("/a/../b/")` → `"/b/"` | 同上 | |
| `.../a_dotdot_join_b` | `("/a/..","b")` → `"/b"` | 同上 | |
| `.../a_dotdot_join_slash` | `("/a/..","/")` → `"/"` | 同上 | |
| `.../a_dotdot_join_b_slash` | `("/a/..","b/")` → `"/b/"` | 同上 | |
| `.../a_dotdot_join_slashb` | `("/a/..","/b")` → `"/b"` | 同上 | |
| `.../a_dot_join_b` | `("/a/.","b")` → `"/a/b"` | 同上 | |
| `.../a_dot_join_dot` | `("/a/.",".")` → `"/a"` | 同上 | |
| `.../a_b_c` | `("a","b","c")` → `"a/b/c"` | 同上 | |
| `.../a_b_absc` | `("a","b","/c")` → `"/c"` | 同上 | |
| `.../a_b_dotdotc` | `("a","b","../c")` → `"a/c"` | 同上 | |

### `TestGetNormalizedAbsolutePath`

> ~100 断言；逐条列。input 为 `(path, currentDirectory)`。

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `gnap/root` | `("/","")` → `"/"` | `path_test.go:TestGetNormalizedAbsolutePath` | |
| `.../root_dot` | `("/.","")` → `"/"` | 同上 | |
| `.../root_dot_slash` | `("/./","")` → `"/"` | 同上 | |
| `.../root_dotdot_slash` | `("/../","")` → `"/"` | 同上 | |
| `.../a` | `("/a","")` → `"/a"` | 同上 | |
| `.../a_slash` | `("/a/","")` → `"/a"` | 同上 | |
| `.../a_dot` | `("/a/.","")` → `"/a"` | 同上 | |
| `.../a_foo_dot` | `("/a/foo.","")` → `"/a/foo."` | 同上 | |
| `.../a_dot_slash` | `("/a/./","")` → `"/a"` | 同上 | |
| `.../a_dot_b` | `("/a/./b","")` → `"/a/b"` | 同上 | |
| `.../a_dot_b_slash` | `("/a/./b/","")` → `"/a/b"` | 同上 | |
| `.../a_dotdot` | `("/a/..","")` → `"/"` | 同上 | |
| `.../a_dotdot_slash` | `("/a/../","")` → `"/"` | 同上 | |
| `.../a_dotdot_b` | `("/a/../b","")` → `"/b"` | 同上 | |
| `.../a_dotdot_b_slash` | `("/a/../b/","")` → `"/b"` | 同上 | |
| `.../a_dotdot_cd_slash` | `("/a/..","/")` → `"/"` | 同上 | |
| `.../a_dotdot_cd_bslash` | `("/a/..","b/")` → `"/"` | 同上 | |
| `.../a_dotdot_cd_slashb` | `("/a/..","/b")` → `"/"` | 同上 | |
| `.../a_dot_cd_b` | `("/a/.","b")` → `"/a"` | 同上 | |
| `.../a_dot_cd_dot` | `("/a/.",".")` → `"/a"` | 同上 | |
| `.../bslash_root` | `("\\","")` → `"/"` | 同上 | |
| `.../bslash_dot` | `("\\.","")` → `"/"` | 同上 | |
| `.../bslash_dot_bslash` | `("\\.\\","")` → `"/"` | 同上 | |
| `.../bslash_dotdot_bslash` | `("\\..\\","")` → `"/"` | 同上 | |
| `.../bslash_a_dot_bslash` | `("\\a\\.\\","")` → `"/a"` | 同上 | |
| `.../bslash_a_dot_b` | `("\\a\\.\\b","")` → `"/a/b"` | 同上 | |
| `.../bslash_a_dot_b_bslash` | `("\\a\\.\\b\\","")` → `"/a/b"` | 同上 | |
| `.../bslash_a_dotdot` | `("\\a\\..","")` → `"/"` | 同上 | |
| `.../bslash_a_dotdot_bslash` | `("\\a\\..\\","")` → `"/"` | 同上 | |
| `.../bslash_a_dotdot_b` | `("\\a\\..\\b","")` → `"/b"` | 同上 | |
| `.../bslash_a_dotdot_b_bslash` | `("\\a\\..\\b\\","")` → `"/b"` | 同上 | |
| `.../bslash_a_dotdot_cd_bslash` | `("\\a\\..","\\")` → `"/"` | 同上 | |
| `.../bslash_a_dotdot_cd_bbslash` | `("\\a\\..","b\\")` → `"/"` | 同上 | |
| `.../bslash_a_dotdot_cd_bslashb` | `("\\a\\..","\\b")` → `"/"` | 同上 | |
| `.../bslash_a_dot_cd_b` | `("\\a\\.","b")` → `"/a"` | 同上 | |
| `.../bslash_a_dot_cd_dot` | `("\\a\\.",".")` → `"/a"` | 同上 | |
| `.../rel_empty` | `("","")` → `""` | 同上 | |
| `.../rel_dot` | `(".","")` → `""` | 同上 | |
| `.../rel_dot_slash` | `("./","")` → `""` | 同上 | |
| `.../rel_dotdot` | `("..","")` → `".."`（不归一为空） | 同上 | |
| `.../rel_dotdot_slash` | `("../","")` → `".."` | 同上 | |
| `.../cd_home_empty` | `("","/home")` → `"/home"` | 同上 | |
| `.../cd_home_dot` | `(".","/home")` → `"/home"` | 同上 | |
| `.../cd_home_dot_slash` | `("./","/home")` → `"/home"` | 同上 | |
| `.../cd_home_dotdot` | `("..","/home")` → `"/"` | 同上 | |
| `.../cd_home_dotdot_slash` | `("../","/home")` → `"/"` | 同上 | |
| `.../a_cd_b` | `("a","b")` → `"b/a"` | 同上 | |
| `.../a_cd_bc` | `("a","b/c")` → `"b/c/a"` | 同上 | |
| `.../dot_a` | `(".a","")` → `".a"` | 同上 | |
| `.../dotdot_a` | `("..a","")` → `"..a"` | 同上 | |
| `.../a_dot_base` | `("a.","")` → `"a."` | 同上 | |
| `.../a_dotdot_base` | `("a..","")` → `"a.."` | 同上 | |
| `.../base_dot_dota` | `("/base/./.a","")` → `"/base/.a"` | 同上 | |
| `.../base_dotdot_dota` | `("/base/../.a","")` → `"/.a"` | 同上 | |
| `.../base_dot_dotdota` | `("/base/./..a","")` → `"/base/..a"` | 同上 | |
| `.../base_dotdot_dotdota` | `("/base/../..a","")` → `"/..a"` | 同上 | |
| `.../base_dot_dotdota_b` | `("/base/./..a/b","")` → `"/base/..a/b"` | 同上 | |
| `.../base_dotdot_dotdota_b` | `("/base/../..a/b","")` → `"/..a/b"` | 同上 | |
| `.../base_dot_a_dot` | `("/base/./a.","")` → `"/base/a."` | 同上 | |
| `.../base_dotdot_a_dot` | `("/base/../a.","")` → `"/a."` | 同上 | |
| `.../base_dot_a_dotdot` | `("/base/./a..","")` → `"/base/a.."` | 同上 | |
| `.../base_dotdot_a_dotdot` | `("/base/../a..","")` → `"/a.."` | 同上 | |
| `.../base_dot_a_dotdot_b` | `("/base/./a../b","")` → `"/base/a../b"` | 同上 | |
| `.../base_dotdot_a_dotdot_b` | `("/base/../a../b","")` → `"/a../b"` | 同上 | |
| `.../a_dotdot_empty2` | `("a/..","")` → `""` | 同上 | |
| `.../a_dslash` | `("/a//","")` → `"/a"` | 同上 | |
| `.../dslash_a_cd_a` | `("//a","a")` → `"//a/"` | 同上 | |
| `.../slash_bslash` | `("/\\","")` → `"//"` | 同上 | |
| `.../a_tslash_cd_a` | `("a///","a")` → `"a/a"` | 同上 | |
| `.../slash_dot_dslash` | `("/.//","")` → `"/"` | 同上 | |
| `.../dslash_dbslash` | `("//\\\\","")` → `"///"` | 同上 | |
| `.../dslash_a_cd_dot` | `(".//a",".")` → `"a"` | 同上 | |
| `.../a_dotdot_dotdot` | `("a/../..","")` → `".."` | 同上 | |
| `.../dotdot_dotdot_cd_bslasha` | `("../..","\\a")` → `"/"` | 同上 | |
| `.../a_colon_cd_b` | `("a:","b")` → `"a:/"` | 同上 | |
| `.../a_dotdot_dotdot_cd_dotdot` | `("a/../..","..")` → `"../.."` | 同上 | |
| `.../a_dotdot_dotdot_cd_b` | `("a/../..","b")` → `""` | 同上 | |
| `.../a_dslash_dotdot_dotdot_cd_dotdot` | `("a//../..","..")` → `"../.."` | 同上 | |
| `.../a_dslash_b` | `("a//b","")` → `"a/b"` | 同上 | |
| `.../a_tslash_b` | `("a///b","")` → `"a/b"` | 同上 | |
| `.../a_b_dslash_c` | `("a/b//c","")` → `"a/b/c"` | 同上 | |
| `.../slasha_b_dslash_c` | `("/a/b//c","")` → `"/a/b/c"` | 同上 | |
| `.../dslasha_b_dslash_c` | `("//a/b//c","")` → `"//a/b/c"` | 同上 | |
| `.../a_dbslash_b` | `("a\\\\b","")` → `"a/b"` | 同上 | |
| `.../a_tbslash_b` | `("a\\\\\\b","")` → `"a/b"` | 同上 | |
| `.../a_b_dbslash_c` | `("a\\b\\\\c","")` → `"a/b/c"` | 同上 | |
| `.../bslasha_b_dbslash_c` | `("\\a\\b\\\\c","")` → `"/a/b/c"` | 同上 | |
| `.../dbslasha_b_dbslash_c` | `("\\\\a\\b\\\\c","")` → `"//a/b/c"` | 同上 | |
| `.../a_slashbslash_b` | `("a/\\b","")` → `"a/b"` | 同上 | |
| `.../a_bslashslash_b` | `("a\\/b","")` → `"a/b"` | 同上 | |
| `.../a_bslashslashbslash_b` | `("a\\/\\b","")` → `"a/b"` | 同上 | |
| `.../a_bslashb_dslash_c` | `("a\\b//c","")` → `"a/b/c"` | 同上 | |

### `TestGetNormalizedAbsolutePathWithoutRoot`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `gnap_wo_root/posix` | `("/a/b/c.txt","/a/b")` → `"a/b/c.txt"` | `path_test.go:TestGetNormalizedAbsolutePathWithoutRoot` | |
| `.../dos_same` | `("c:/work/hello.txt","c:/work")` → `"work/hello.txt"` | 同上 | |
| `.../dos_diff` | `("c:/work/hello.txt","d:/worspaces")` → `"work/hello.txt"` | 同上 | |

### `TestGetRelativePathToDirectoryOrUrl`

> input 为 `(dir, path, isAbsolutePathAnUrl=false, ComparePathsOptions{})`。

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `relurl/root_root` | `("/","/")` → `""` | `path_test.go:TestGetRelativePathToDirectoryOrUrl` | |
| `.../a_a` | `("/a","/a")` → `""` | 同上 | |
| `.../a_slash_a` | `("/a/","/a")` → `""` | 同上 | |
| `.../a_root` | `("/a","/")` → `".."` | 同上 | |
| `.../a_b` | `("/a","/b")` → `"../b"` | 同上 | |
| `.../ab_b` | `("/a/b","/b")` → `"../../b"` | 同上 | |
| `.../abc_b` | `("/a/b/c","/b")` → `"../../../b"` | 同上 | |
| `.../abc_bc` | `("/a/b/c","/b/c")` → `"../../../b/c"` | 同上 | |
| `.../abc_ab` | `("/a/b/c","/a/b")` → `".."` | 同上 | |
| `.../c_d_volumes` | `("c:","d:")` → `"d:/"` | 同上 | |
| `.../file_root_root` | `("file:///","file:///")` → `""` | 同上 | |
| `.../file_a_a` | `("file:///a","file:///a")` → `""` | 同上 | |
| `.../file_a_slash_a` | `("file:///a/","file:///a")` → `""` | 同上 | |
| `.../file_a_root` | `("file:///a","file:///")` → `".."` | 同上 | |
| `.../file_a_b` | `("file:///a","file:///b")` → `"../b"` | 同上 | |
| `.../file_ab_b` | `("file:///a/b","file:///b")` → `"../../b"` | 同上 | |
| `.../file_abc_b` | `("file:///a/b/c","file:///b")` → `"../../../b"` | 同上 | |
| `.../file_abc_bc` | `("file:///a/b/c","file:///b/c")` → `"../../../b/c"` | 同上 | |
| `.../file_abc_ab` | `("file:///a/b/c","file:///a/b")` → `".."` | 同上 | |
| `.../file_c_d` | `("file:///c:","file:///d:")` → `"file:///d:/"` | 同上 | |

### `TestToFileNameLowerCase`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `lower/ascii` | `"/user/UserName/projects/Project/file.ts"` → `"/user/username/projects/project/file.ts"` | `path_test.go:TestToFileNameLowerCase` | |
| `.../sharp_s` | `"/user/UserName/projects/projectß/file.ts"` → `"/user/username/projects/projectß/file.ts"` | 同上 | |
| `.../I_with_dot` | `"/user/UserName/projects/İproject/file.ts"` → `"/user/username/projects/İproject/file.ts"`（`\u0130` 不转） | 同上 | |
| `.../dotless_i` | `"/user/UserName/projects/ı/file.ts"` → `"/user/username/projects/ı/file.ts"` | 同上 | |

### `TestToPath`

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `to_path/rel_insensitive` | `("file.ext","path/to",false)` → `"path/to/file.ext"` | `path_test.go:TestToPath` | |
| `.../abs_sensitive` | `("file.ext","/path/to",true)` → `"/path/to/file.ext"` | 同上 | |
| `.../abs_dotdot` | `("/path/to/../file.ext","path/to",true)` → `"/path/file.ext"` | 同上 | |

### `TestPathIsRelative`（表驱动 `t.Run`，含 `init()` 把 `/` 替换为 `\` 复制一份）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `path_is_relative/dot` | `"."` → true | `path_test.go:TestPathIsRelative/.` | |
| `.../dotdot` | `".."` → true | `.../..` | |
| `.../dot_slash` | `"./"` → true | `..././` | |
| `.../dotdot_slash` | `"../"` → true | `.../../` | |
| `.../dot_foo_bar` | `"./foo/bar"` → true | `.../​./foo/bar` | |
| `.../dotdot_foo_bar` | `"../foo/bar"` → true | `.../../foo/bar` | |
| `.../dotdot_long` | `"../" + "foo/"*100` → true | `.../../foo/...etc` | |
| `.../empty` | `""` → false | `.../(empty)` | |
| `.../foo` | `"foo"` → false | `.../foo` | |
| `.../foo_bar` | `"foo/bar"` → false | `.../foo/bar` | |
| `.../slash_foo_bar` | `"/foo/bar"` → false | `.../​/foo/bar` | |
| `.../c_foo_bar` | `"c:/foo/bar"` → false | `.../c:/foo/bar` | |
| `.../bslash_variants` | 上述各项把 `/`→`\` 的复制（如 `".\\"` → true、`"..\\foo\\bar"` → true 等） | `path_test.go:TestPathIsRelative`（init 复制集） | |

### `TestGetCommonParents`（表驱动 `t.Run`）

| Rust 测试 | input(paths, minComponents) → (parents, ignored) | Go 对照 | 完成 |
|---|---|---|---|
| `common_parents/empty` | `([], 1)` → `(nil, ignored{})` | `path_test.go:TestGetCommonParents/empty input` | |
| `.../single` | `(["/a/b/c/d"], 1)` → `(["/a/b/c/d"], {})` | `.../single path returns itself` | |
| `.../short_ignored` | `(["/a/b/c/d","/a/b/c/e","/a/b/f/g","/x/y"], 4)` → `(["/a/b/c","/a/b/f/g"], {"/x/y"})` | `.../paths shorter than minComponents are ignored` | |
| `.../three_share_ab` | `(["/a/b/c/d","/a/b/c/e","/a/b/f/g"], 1)` → `(["/a/b"], {})` | `.../three paths share /a/b` | |
| `.../mixed_collapse_root` | `([...,"/x/y/z"], 1)` → `(["/"], {})` | `.../mixed with short path collapses to root when minComponents=1` | |
| `.../mixed_preserve_min3` | `([...,"/x/y/z"], 3)` → `(["/a/b","/x/y/z"], {})` | `.../mixed with short path preserves both when minComponents=3` | |
| `.../diff_volumes` | `(["c:/a/b/c/d","d:/a/b/c/d"], 1)` → `(["c:/a/b/c/d","d:/a/b/c/d"], {})` | `.../different volumes are returned individually` | |
| `.../duplicate_dedup` | `(["/a/b/c/d","/a/b/c/d"], 1)` → `(["/a/b/c/d"], {})` | `.../duplicate paths deduplicate result` | |
| `.../few_components_asis` | `(["/a/b/c/d","/x/y"], 2)` → `(["/a/b/c/d","/x/y"], {})` | `.../paths with few components are returned as-is when minComponents met` | |
| `.../min2` | `(["/a/b/c/d","/a/z/c/e","/a/aaa/f/g","/x/y/z"], 2)` → `(["/a","/x/y/z"], {})` | `.../minComponents=2` | |
| `.../trailing_seps` | `(["/a/b/","/a/b/c"], 1)` → `(["/a/b"], {})` | `.../trailing separators are handled` | |

## `ignoredpaths_test.go`

### `TestContainsIgnoredPath`（表驱动）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `ignored/node_modules_dot` | `"/project/node_modules/.pnpm/file.ts"` → true | `ignoredpaths_test.go:TestContainsIgnoredPath/node_modules dot path` | |
| `.../git_dir` | `"/project/.git/hooks/pre-commit"` → true | `.../git directory` | |
| `.../emacs_lock` | `"/project/src/file.ts.#"` → true | `.../emacs lock file` | |
| `.../regular` | `"/project/src/file.ts"` → false | `.../regular file path` | |
| `.../nm_no_dot` | `"/project/node_modules/lodash/index.js"` → false | `.../node_modules without dot` | |
| `.../empty` | `""` → false | `.../empty path` | |
| `.../multiple_patterns` | `"/project/node_modules/.pnpm/.git/.#file.ts"` → true | `.../path with multiple ignored patterns` | |
| `.../case_sensitive` | `"/project/NODE_MODULES/.PNPM/file.ts"` → false | `.../case sensitive test` | |
| `.../middle` | `"/project/src/node_modules/.pnpm/dist/file.js"` → true | `.../path with ignored pattern in middle` | |
| `.../at_end` | `"/project/src/file.ts.#"` → true | `.../path with ignored pattern at end` | |

### `TestIgnoredPathsPatterns`

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `ignored_patterns_present` | `["/node_modules/.","/.git",".#"]` 各自插入路径均被检出 | `ignoredpaths_test.go:TestIgnoredPathsPatterns` | |

### `TestIgnoredPathsEdgeCases`（表驱动）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `ignored_edge/start` | `"/node_modules./file.ts"` → false（模式是 `/node_modules/.` 非 `/node_modules.`） | `ignoredpaths_test.go:TestIgnoredPathsEdgeCases/pattern at start` | |
| `.../end` | `"/project/file.ts.#"` → true | `.../pattern at end` | |
| `.../multiple` | `"/project/.git/node_modules./.git/file.ts"` → true | `.../multiple occurrences` | |
| `.../no_slashes` | `"node_modules.file.ts"` → false | `.../no slashes` | |
| `.../single_slash` | `"/file.ts"` → false | `.../single slash` | |

## `startsWithDirectory_test.go`

### `TestStartsWithDirectory`（表驱动）

| Rust 测试 | input(file, dir, caseSensitive) → expected | Go 对照 | 完成 |
|---|---|---|---|
| `swd/exact_sensitive` | `("/project/src/file.ts","/project/src",true)` → true | `startsWithDirectory_test.go:TestStartsWithDirectory/exact match case sensitive` | |
| `.../exact_insensitive` | `("/project/src/file.ts","/PROJECT/SRC",false)` → true | `.../exact match case insensitive` | |
| `.../sensitive_mismatch` | `("/project/src/file.ts","/PROJECT/SRC",true)` → false | `.../case sensitive mismatch` | |
| `.../not_in_dir` | `("/project/lib/file.ts","/project/src",true)` → false | `.../file not in directory` | |
| `.../subdir` | `("/project/src/components/Button.tsx","/project/src",true)` → true | `.../file in subdirectory` | |
| `.../parent_dir` | `("/project/file.ts","/project/src",true)` → false | `.../file in parent directory` | |
| `.../windows_seps` | `("C:\\project\\src\\file.ts","C:\\project\\src",true)` → true | `.../windows style separators` | |
| `.../mixed_seps` | `("/project/src/file.ts","\\project\\src",true)` → false | `.../mixed separators` | |
| `.../empty_dir` | `("/project/src/file.ts","",true)` → false | `.../empty directory name` | |
| `.../empty_file` | `("","/project/src",true)` → false | `.../empty file name` | |
| `.../identical` | `("/project/src","/project/src",true)` → false | `.../identical paths` | |
| `.../dir_trailing_sep` | `("/project/src/file.ts","/project/src/",true)` → true | `.../directory with trailing separator` | |
| `.../unicode` | `("/project/测试/file.ts","/project/测试",true)` → true | `.../unicode characters` | |
| `.../unicode_insensitive` | `("/project/测试/file.ts","/PROJECT/测试",false)` → true | `.../unicode case insensitive` | |

### `TestStartsWithDirectoryEdgeCases`（表驱动）

| Rust 测试 | input(file, dir, caseSensitive) → expected | Go 对照 | 完成 |
|---|---|---|---|
| `swd_edge/file_shorter` | `("/proj","/project",true)` → false | `startsWithDirectory_test.go:TestStartsWithDirectoryEdgeCases/file name shorter than directory` | |
| `.../prefix_no_sep` | `("/projectsrc/file.ts","/project",true)` → false | `.../file name starts with directory but no separator` | |
| `.../relative` | `("src/file.ts","src",true)` → true | `.../relative paths` | |
| `.../abs_vs_rel` | `("/project/src/file.ts","project/src",true)` → false | `.../absolute vs relative` | |

## `untitled_test.go`

### `TestUntitledPathHandling`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `untitled_root_length` | `get_encoded_root_length` 返回 2 | `"^/untitled/ts-nul-authority/Untitled-2"` → 2 | `untitled_test.go:TestUntitledPathHandling` | |
| `untitled_is_rooted` | `is_rooted_disk_path` 为真 | 同上 → true | 同上 | |
| `untitled_to_path_no_resolve` | `to_path` 不对 untitled 解析 | `(untitled, "/home/user/project", true)` → `"^/untitled/ts-nul-authority/Untitled-2"` | 同上 | |
| `untitled_gnap_no_resolve` | `get_normalized_absolute_path` 不解析 | `(untitled, "/home/user/project")` → `"^/untitled/ts-nul-authority/Untitled-2"` | 同上 | |

### `TestUntitledPathEdgeCases`（表驱动）

| Rust 测试 | input → (encodedRootLen, isRooted) | Go 对照 | 完成 |
|---|---|---|---|
| `untitled_edge/minimal` | `"^/"` → (2, true) | `untitled_test.go:TestUntitledPathEdgeCases/^/` | |
| `.../normal` | `"^/untitled/ts-nul-authority/test"` → (2, true) | `.../^/untitled/ts-nul-authority/test` | |
| `.../just_caret` | `"^"` → (0, false) | `.../^` | |
| `.../caret_x` | `"^x"` → (0, false) | `.../^x` | |
| `.../double_caret` | `"^^/"` → (0, false) | `.../^^/` | |
| `.../x_caret` | `"x^/"` → (0, false) | `.../x^/` | |
| `.../deeper` | `"^/untitled/ts-nul-authority/path/with/deeper/structure"` → (2, true) | `.../^/untitled/...` | |

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（24 个全覆盖：path 17 + ignoredpaths 3 + startsWithDirectory 2 + untitled 2）
- [x] 表驱动子用例逐行列出（ContainsIgnoredPath/IgnoredPathsEdgeCases/StartsWithDirectory*/PathIsRelative/GetCommonParents/UntitledPathEdgeCases）
- [x] 命令式 `assert.Equal` 序列逐断言列出（GetRootLength/GetDirectoryPath/GetPathComponents/ResolvePath/GetNormalizedAbsolutePath 等）
- [x] expected 值均取自 Go 测试字面量
- [x] 每条带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐：每个被测函数在 impl.md 均有实现 TODO

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `FuzzGetNormalizedAbsolutePath`（对拍 `getNormalizedAbsolutePath_old`） | 转 Rust property test（proptest），实现期补 | 实现期 / P10 |
| `FuzzToFileNameLowerCase`（对拍正则版 `oldToFileNameLowerCase`） | 同上 | 实现期 / P10 |
| `FuzzHasRelativePathSegment`（对拍正则版） | 同上 | 实现期 / P10 |
| `GetCommonParents` 在 program/project 的真实集成 | 需上层包 | P6/P8 + P10 |
| 路径工具在真实 module resolution 的端到端一致 | 需 module（P4） | P10 parity |
