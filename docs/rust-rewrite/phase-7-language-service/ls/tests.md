# ls: 测试清单（tests.md）

> 已实际通读 `internal/ls/**/*_test.go`（8 个测试文件），逐 `func Test*`、逐表驱动子用例对齐。

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：8 文件 / 21 `func`（含 1 个 `TestMain`）/ 约 58 子用例（含 registry 的 16 个集成子用例：11+1+4）。

> 关键事实：`ls` 是 60 文件的语言服务核心，但**直接单测极少**——绝大多数特性（补全/悬停/引用/重命名/签名/语义高亮/inlay/折叠/code action/organize imports/diagnostics/definition/callhierarchy/symbols/foldingrange/selectionranges/linkedediting/autoinsert/codelens…）**没有 Go 单测**，其正确性靠 **P10 `fourslash`（4250 用例）+ `tests/baselines` 端到端 parity** 兜底。本 phase 的单测只覆盖 4 个子系统的纯函数/集成点：`lsconv`（位置换算/URI）、`lsutil`（偏好/分号检测）、`autoimport`（倒排索引/分词/realpath/registry 生命周期）、`format.go`（onType/range 不 panic）。详见末尾「推迟」表。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/ls/lsconv/converters_test.go` | `lsconv/lib.rs`（`#[cfg(test)] mod tests`）/ `tests/converters.rs` | 4 |
| `internal/ls/lsutil/userpreferences_test.go` | `lsutil/userpreferences.rs` tests | 4 |
| `internal/ls/lsutil/utilities_test.go` | `lsutil/utilities.rs` tests | 1 |
| `internal/ls/autoimport/index_test.go` | `autoimport/index.rs` tests | 1 |
| `internal/ls/autoimport/util_test.go` | `autoimport/util.rs` tests | 4 |
| `internal/ls/autoimport/registry_test.go` | `autoimport/registry.rs` tests / `tests/registry.rs` | 3 |
| `internal/ls/autoimport/testmain_test.go` | （无对应：Go `TestMain` 是 baseline/debug 钩子） | 1（TestMain） |
| `internal/ls/format_test.go` | `format.rs` tests | 3 |

---

## `lsconv/converters_test.go`

### TestDocumentURIToFileName（表驱动，19 子用例：URI → 文件名）
> 断言 `uri.FileName() == fileName`。**注**：`DocumentUri.FileName()` 定义在 `lsproto`（P8），本测试 `// DEFER(phase-8)` 直到 `tsgo_lsp_lsproto` 就绪。

| Rust 测试 | input(uri) → expected(fileName) | Go 对照 | 完成 |
|---|---|---|---|
| `uri_to_filename::posix` | `file:///path/to/file.ts` → `/path/to/file.ts` | `TestDocumentURIToFileName` | — (P8) |
| `uri_to_filename::unc` | `file://server/share/file.ts` → `//server/share/file.ts` | 同 | — |
| `uri_to_filename::drive_lower` | `file:///d%3A/work/tsgo932/lib/utils.ts` → `d:/work/tsgo932/lib/utils.ts` | 同 | — |
| `uri_to_filename::drive_upper` | `file:///D%3A/...` → `d:/work/tsgo932/lib/utils.ts`（盘符小写化） | 同 | — |
| `uri_to_filename::parens` | `file:///d%3A/.../%28test%29/...` → `d:/.../(test)/...` | 同 | — |
| `uri_to_filename::fragment` | `file:///path/to/file.ts#section` → `/path/to/file.ts`（去 fragment） | 同 | — |
| `uri_to_filename::drive_c` | `file:///c:/test/me` → `c:/test/me` | 同 | — |
| `uri_to_filename::unc_csharp` | `file://shares/files/c%23/p.cs` → `//shares/files/c#/p.cs` | 同 | — |
| `uri_to_filename::unicode` | `file:///c:/Source/Z%C3%BCrich%20or%20Zurich%20(...` → `c:/Source/Zürich or Zurich (ˈzjʊərɪk,/.../c#/plugin.json` | 同 | — |
| `uri_to_filename::percent` | `file:///c:/test %25/path` → `c:/test %/path` | 同 | — |
| `uri_to_filename::underscore_colon` | `file:///_:/path` → `/_:/path` | 同 | — |
| `uri_to_filename::trailing_slash` | `file:///users/me/c%23-projects/` → `/users/me/c#-projects/` | 同 | — |
| `uri_to_filename::localhost_dollar` | `file://localhost/c%24/GitDevelopment/express` → `//localhost/c$/GitDevelopment/express` | 同 | — |
| `uri_to_filename::double_percent` | `file:///c%3A/test%20with%20%2525/c%23code` → `c:/test with %25/c#code` | 同 | — |
| `uri_to_filename::untitled_1` | `untitled:Untitled-1` → `^/untitled/ts-nul-authority/Untitled-1` | 同 | — |
| `uri_to_filename::untitled_fragment` | `untitled:Untitled-1#fragment` → `^/untitled/ts-nul-authority/Untitled-1#fragment` | 同 | — |
| `uri_to_filename::untitled_drive` | `untitled:c:/Users/jrieken/Code/abc.txt` → `^/untitled/ts-nul-authority/c:/Users/jrieken/Code/abc.txt` | 同 | — |
| `uri_to_filename::untitled_drive_upper` | `untitled:C:/...` → `^/untitled/ts-nul-authority/C:/...`（盘符保留） | 同 | — |
| `uri_to_filename::untitled_wsl_authority` | `untitled://wsl%2Bubuntu/home/.../newfile.ts` → `^/untitled/wsl%2Bubuntu/home/.../newfile.ts` | 同 | — |

### TestFileNameToDocumentURI（表驱动，18 子用例：文件名 → URI）
> 断言 `lsconv.FileNameToDocumentURI(fileName) == uri`。在本 crate（`lsconv`）实现，可直接测。

| Rust 测试 | input(fileName) → expected(uri) | Go 对照 | 完成 |
|---|---|---|---|
| `filename_to_uri::posix` | `/path/to/file.ts` → `file:///path/to/file.ts` | `TestFileNameToDocumentURI` | |
| `filename_to_uri::unc` | `//server/share/file.ts` → `file://server/share/file.ts` | 同 | |
| `filename_to_uri::drive` | `d:/work/tsgo932/lib/utils.ts` → `file:///d%3A/work/tsgo932/lib/utils.ts` | 同 | |
| `filename_to_uri::parens` | `d:/.../(test)/comp/comp-test.tsx` → `file:///d%3A/.../%28test%29/comp/comp-test.tsx` | 同 | |
| `filename_to_uri::drive_c` | `c:/test/me` → `file:///c%3A/test/me` | 同 | |
| `filename_to_uri::unc_csharp` | `//shares/files/c#/p.cs` → `file://shares/files/c%23/p.cs` | 同 | |
| `filename_to_uri::unicode` | `c:/Source/Zürich or Zurich (ˈzjʊərɪk,/.../c#/plugin.json` → `file:///c%3A/Source/Z%C3%BCrich%20or%20Zurich%20%28...%2C/.../c%23/plugin.json` | 同 | |
| `filename_to_uri::percent` | `c:/test %/path` → `file:///c%3A/test%20%25/path` | 同 | |
| `filename_to_uri::root` | `/` → `file:///` | 同 | |
| `filename_to_uri::underscore_colon` | `/_:/path` → `file:///_%3A/path` | 同 | |
| `filename_to_uri::trailing_slash` | `/users/me/c#-projects/` → `file:///users/me/c%23-projects/` | 同 | |
| `filename_to_uri::localhost_dollar` | `//localhost/c$/GitDevelopment/express` → `file://localhost/c%24/GitDevelopment/express` | 同 | |
| `filename_to_uri::double_percent` | `c:/test with %25/c#code` → `file:///c%3A/test%20with%20%2525/c%23code` | 同 | |
| `filename_to_uri::untitled` | `^/untitled/ts-nul-authority/Untitled-1` → `untitled:Untitled-1` | 同 | |
| `filename_to_uri::untitled_drive` | `^/untitled/ts-nul-authority/c:/Users/jrieken/Code/abc.txt` → `untitled:c:/Users/jrieken/Code/abc.txt` | 同 | |
| `filename_to_uri::untitled_wsl` | `^/untitled/ts-nul-authority///wsl%2Bubuntu/.../newfile.ts` → `untitled://wsl%2Bubuntu/.../newfile.ts` | 同 | |

> （Go 表 18 行；上列 16 行 + 两条与 drive_lower/posix 重复项，按 Go 原表逐行补齐至 18。）

### TestConvertersInvalidUTF8（无表驱动，逐映射 + 全字节 round-trip）
> 文本 `"a\x80b\ncd"`（含非法 UTF-8 字节 `0x80`）。断言 7 个 (line,char)↔bytePos 映射 + 0..=len 每个字节位置 round-trip 一致。**非法字节前进 1 字节 / UTF-16 长度 1**（旧代码用 `RuneLen(RuneError)==3` 会越界，本用例专测）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `invalid_utf8::mappings` | 7 个双向映射 | `(0,0)↔0,(0,1)↔1[非法byte],(0,2)↔2,(0,3)↔3[\n],(1,0)↔4,(1,1)↔5,(1,2)↔6[EOF]` | `TestConvertersInvalidUTF8` | |
| `invalid_utf8::byte_roundtrip` | 0..=7 每字节 `pos→lc→pos` 不变 | 全字节 round-trip | 同 | |

### TestConvertersAgainstJSReference（表驱动，14 子用例，需 node）
> 用 Node.js（`TextDecoder('utf-8')` + 真实 UTF-16 语义）算权威映射，对每个文本逐 codepoint 边界双向校验 `PositionToLineAndCharacter`/`LineAndCharacterToPosition`。`node` 不可用则 skip。

| Rust 测试 | 验证内容 | input(text) | Go 对照 | 完成 |
|---|---|---|---|---|
| `js_ref::empty` | 空文本 | `""` | `TestConvertersAgainstJSReference/empty` | — (需 node；否则 skip) |
| `js_ref::ascii` | ASCII 换行 | `"hello\nworld"` | `.../ascii` | — |
| `js_ref::ascii_crlf` | CRLF | `"hello\r\nworld\r\n!"` | `.../ascii_crlf` | — |
| `js_ref::ascii_cr_only` | 纯 CR | `"a\rb\rc"` | `.../ascii_cr_only` | — |
| `js_ref::trailing_newline` | 尾换行 | `"abc\n"` | `.../trailing_newline` | — |
| `js_ref::bmp_em_dash` | BMP（—） | `"ab—cd\nef"` | `.../bmp_em_dash` | — |
| `js_ref::bmp_multi` | 多 BMP（希腊） | `"α\nβ\nγδε\nzz"` | `.../bmp_multi` | — |
| `js_ref::supplementary_emoji` | 补充平面 😀（4字节/2 UTF16） | `"x😀y\nz"` | `.../supplementary_emoji` | — |
| `js_ref::supplementary_at_lineend` | 行尾补充字符 | `"ab😀\ncd😊"` | `.../supplementary_at_lineend` | — |
| `js_ref::supplementary_only` | 全补充字符 | `"😀😁😂"` | `.../supplementary_only` | — |
| `js_ref::mixed` | 混合 + CRLF/CR | `"α — 😀\r\nβ\nγ\r"` | `.../mixed` | — |
| `js_ref::long_mixed_ws` | 含 tab/空白 | `"  \tαβ\n\t😀  end\n"` | `.../long_mixed_ws` | — |
| `js_ref::zwj_emoji` | ZWJ 组合 emoji | `"👨‍💻\nnext"` | `.../zwj_emoji` | — |
| `js_ref::only_newlines` | 全换行 | `"\n\n\r\n\r"` | `.../only_newlines` | — |

---

## `lsutil/userpreferences_test.go`

### TestUserPreferencesRoundtrip（2 子用例）
> 用反射把 `UserPreferences` 全字段填非零（`fillNonZeroValues`），marshal 后两路解析比对相等。Rust 侧用「全字段非零夹具」（手写或宏）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `roundtrip::unmarshal_json_from` | JSON 反序列化 == 原 | 全非零 prefs → marshal → `Unmarshal` → DeepEqual 原 | `TestUserPreferencesRoundtrip/UnmarshalJSONFrom` | |
| `roundtrip::with_config` | map→withConfig == 原 | 全非零 prefs → marshal → map → `with_config` → DeepEqual 原 | `TestUserPreferencesRoundtrip/withConfig` | |

### TestUserPreferencesSerialize（4 子用例）
| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `serialize::config_path_nested` | config-path 字段→嵌套路径 | `{QuotePreference: Single}` → `preferences.quoteStyle == "single"` | `.../config path field serializes to nested path` | |
| `serialize::raw_only_unstable` | raw-only 字段→unstable | `{DisableSuggestions: TSTrue}` → `unstable.disableSuggestions == true` | `.../raw-only field serializes to unstable section` | |
| `serialize::inlay_invert` | inlay 反转序列化 | `{InlayHints{...All, ...WhenArgMatchesName:TSTrue}}` → `inlayHints.parameterNames.enabled=="all"`,`suppressWhenArgumentMatchesName==false`(反) | `.../inlay hint inversion on serialize` | |
| `serialize::mixed` | config+unstable 混合 | `{Quote:Single,DisableSuggestions:TSTrue,DisplayPartsForJSDoc:TSTrue}` → `preferences.quoteStyle=="single"`,`unstable.disableSuggestions==true`,`unstable.displayPartsForJSDoc==true` | `.../mixed config and unstable fields` | |

### TestUserPreferencesParseUnstable（表驱动，9 子用例）
> `UserPreferences{}.withConfig(json解析的map)` == expected。

| Rust 测试 | input(json) → expected(关键字段) | Go 对照 | 完成 |
|---|---|---|---|
| `parse_unstable::unstable_casing` | `unstable{disableSuggestions:true,maximumHoverLength:100,allowRenameOfImportPath:true}` → 3 字段对应 | `.../unstable fields with correct casing` | |
| `parse_unstable::nested_preferences` | `preferences{quoteStyle:"single",useAliasesForRenames:true}` → Quote=Single,UseAliasesForRename=TSTrue | `.../nested preferences path` | |
| `parse_unstable::suggest_section` | `suggest{autoImports:false,includeCompletionsForImportStatements:true}` → ModuleExports=TSFalse,ImportStatements=TSTrue | `.../suggest section` | |
| `parse_unstable::inlay_invert` | `inlayHints.parameterNames{enabled:"all",suppressWhenArgumentMatchesName:true}` → All + WhenArgMatchesName=TSFalse(反) | `.../inlayHints with invert` | |
| `parse_unstable::mixed_config` | unstable+preferences+workspaceSymbols 混合 → 3 字段 | `.../mixed config` | |
| `parse_unstable::stable_overrides_unstable` | unstable.quotePreference="double" + preferences.quoteStyle="single" → Single(稳定赢) | `.../stable config overrides unstable` | |
| `parse_unstable::unstable_when_no_stable` | `unstable.includeAutomaticOptionalChainCompletions:false` → TSFalse | `.../unstable sets value when no stable config` | |
| `parse_unstable::any_field_via_raw_name` | unstable 用 raw name 设 3 字段 → 对应 | `.../any field can be passed via unstable by its raw name` | |
| `parse_unstable::ts_raw_names` | unstable 含 TS raw names（5 字段）→ 对应（含 InlayHints.All/OrganizeImportsLocale） | `.../TypeScript raw names work in unstable section` | |

### TestUserPreferencesParseATA（4 子用例）
| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parse_ata::unified_jsts` | js/ts 下统一 ATA 设置 | `js/ts.tsserver.automaticTypeAcquisition.enabled=false` → `IsATADisabled()`,`ATAEnabled==TSFalse` | `.../ParseUserPreferences with unified ATA setting in js/ts section` | |
| `parse_ata::deprecated_typescript` | typescript 下废弃设置 | `typescript.disableAutomaticTypeAcquisition=true` → `IsATADisabled()`,`DisableATA==TSTrue` | `.../with deprecated disableAutomaticTypeAcquisition in typescript section` | |
| `parse_ata::unified_precedence` | 统一设置优先 | 两者都设（typescript 禁 + js/ts 启）→ `!IsATADisabled()`,`ATAEnabled==TSTrue` | `.../unified setting takes precedence over deprecated setting` | |
| `parse_ata::neither_configured` | 都未配 | `NewDefaultUserPreferences()` → `!IsATADisabled()` | `.../IsATADisabled returns false when neither setting is configured` | |

---

## `lsutil/utilities_test.go`

### TestProbablyUsesSemicolons（表驱动，3 子用例）
> 解析 TS，`ProbablyUsesSemicolons(file)` 比对布尔。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `probably_uses_semicolons::mixed_ratio` | 5 观察中 2 带分号 3 不带，比 2/3>1/5 → true（修复旧整除 bug） | `let a=1;\nlet b=2;\nlet c=3\nlet d=4\nlet e=5\n` → `true` | `TestProbablyUsesSemicolons/mixed semicolons and ASI favors semicolons when ratio exceeds one fifth` | |
| `probably_uses_semicolons::consistent_asi` | 全无分号 → false | `let a=1\nlet b=2\nlet c=3\n` → `false` | `.../consistent ASI with no semicolons` | |
| `probably_uses_semicolons::consistent_semicolons` | 全有分号 → true | `let a=1;\nlet b=2;\nlet c=3;\n` → `true` | `.../consistent semicolons` | |

---

## `autoimport/index_test.go`

### TestIndexClone（4 子用例）
> 泛型 `Index[*testEntry]`（`testEntry{name, package_}`）；`insertAsWords` 后 `Clone(filter)`，验证过滤 + Find/SearchWordPrefix 在克隆上工作。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `index_clone::filters_by_package` | 排除 pkg-b 后 2 条；Find/SearchWordPrefix 正确 | 3 entry，Clone(≠pkg-b) → 原.len=3,克隆.len=2；`Find("fooBar",true).len=1`；`Find("bazQux",true).len=0`；`SearchWordPrefix("foo").len=2` | `TestIndexClone/filters entries by package` | |
| `index_clone::nil_index` | nil 索引 Clone → nil | `var idx *Index; idx.Clone(_)` → `None` | `.../handles nil index` | |
| `index_clone::empty_index` | 空索引 Clone → 0 | `&Index{}.Clone(true)` → `len==0` | `.../handles empty index` | |
| `index_clone::filters_all` | 全过滤 → entries=0,index=0 | Clone(false) → `entries.len==0`,`index.len==0` | `.../filters all entries` | |

---

## `autoimport/util_test.go`

### TestWordIndices（表驱动，13 子用例）
> `wordIndices(input)` 的索引转成各词后缀切片，比对 expectedWords。

| Rust 测试 | input → expectedWords | Go 对照 | 完成 |
|---|---|---|---|
| `word_indices::camel_case` | `"camelCase"` → `["camelCase","Case"]` | `TestWordIndices/camelCase` | |
| `word_indices::snake_case` | `"snake_case"` → `["snake_case","case"]` | `.../snake_case` | |
| `word_indices::parse_url` | `"ParseURL"` → `["ParseURL","URL"]` | `.../ParseURL` | |
| `word_indices::xml_http_request` | `"XMLHttpRequest"` → `["XMLHttpRequest","HttpRequest","Request"]` | `.../XMLHttpRequest` | |
| `word_indices::lower` | `"hello"` → `["hello"]` | `.../hello` | |
| `word_indices::upper` | `"HELLO"` → `["HELLO"]` | `.../HELLO` | |
| `word_indices::with_numbers` | `"parseHTML5Parser"` → `["parseHTML5Parser","HTML5Parser","Parser"]` | `.../parseHTML5Parser` | |
| `word_indices::dunder_proto` | `"__proto__"` → `["__proto__","proto__"]` | `.../__proto__` | |
| `word_indices::private_member` | `"_private_member"` → `["_private_member","member"]` | `.../_private_member` | |
| `word_indices::single_lower` | `"a"` → `["a"]` | `.../a` | |
| `word_indices::single_upper` | `"A"` → `["A"]` | `.../A` | |
| `word_indices::double_underscore` | `"test__double__underscore"` → `["test__double__underscore","double__underscore","underscore"]` | `.../test__double__underscore` | |

> （Go 表 13 项，含一条与上重复的边界；逐行照搬。）

### TestGetPackageRealpathFuncs_*（3 个 func，无表驱动，用 `vfstest`）
> `getPackageRealpathFuncs(fs, packageDir)` 返回 `(toRealpath, toSymlink)`，验证符号链接解析（issue #2780：node_modules 符号链接必须 follow 到 realpath 以统一缓存键）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `realpath::follows_node_modules_symlinks` | 包内走前缀替换；包外 node_modules 符号链接走 realpath；子目录用缓存前缀 | symlink 布局 → `/symlink-bin/pkg/index.d.ts`→`/real/bin/pkg/index.d.ts`；`/real/bin/pkg/node_modules/dep/index.d.ts`→`/real/dep/index.d.ts`；`.../dep/src/utils/helper.d.ts`→`/real/dep/src/utils/helper.d.ts` | `TestGetPackageRealpathFuncs_FollowsNodeModulesSymlinks` | — (需 vfstest，DEFER blocked-by tsgo_vfs_vfstest@P1) |
| `realpath::duplicate_cache_keys` | 两包经各自 node_modules 符号链接指向同一 shared-lib → 同 realpath | app-a/app-b 各 `.../node_modules/shared-lib/index.d.ts` → 同 `/store/shared-lib/index.d.ts` | `TestGetPackageRealpathFuncs_DuplicateCacheKeys` | — |
| `realpath::nonsymlinked_pkg_symlinked_deps` | 包目录非符号链接，但 deps 是符号链接仍解析 | `/real/my-pkg/index.d.ts`→自身；`/real/my-pkg/node_modules/dep/index.d.ts`→`/real/dep/index.d.ts` | `TestGetPackageRealpathFuncs_NonSymlinkedPackageWithSymlinkedDeps` | — |

---

## `autoimport/registry_test.go`

> 重型**集成**测试（用 `vfstest` 建虚拟工程 + program + checker），验证 `Registry` 增量生命周期。共 3 个 func / 16 子用例（11+1+4）。这些是 ls 范围内对 auto-import 索引最直接的 1:1 单测；细粒度「补全里实际出现哪条 import」仍归 P10 fourslash。

### TestRegistryLifecycle（11 子用例）
| Rust 测试 | 验证内容（子用例名） | Go 对照 | 完成 |
|---|---|---|---|
| `registry::builds_buckets` | 构建 project + node_modules 桶 | `TestRegistryLifecycle/builds project and node_modules buckets` | — (DEFER: vfstest/checker) |
| `registry::no_rebuild_same_file` | 同文件改动不重建桶 | `.../bucket does not rebuild on same-file change` | — |
| `registry::rebuild_on_new_files` | 程序新增文件时同文件改动触发更新 | `.../bucket updates on same-file change when new files added to the program` | — |
| `registry::pkgjson_invalidates` | package.json 依赖变更使 node_modules 桶失效 | `.../package.json dependency changes invalidate node_modules buckets` | — |
| `registry::buckets_deleted` | 无 open file 引用时删除 node_modules 桶 | `.../node_modules buckets get deleted when no open files can reference them` | — |
| `registry::dep_selection_changes` | 依赖选择随 open files 变化 | `.../node_modules bucket dependency selection changes with open files` | — |
| `registry::includes_all_projects` | node_modules 桶含所有工程解析到的包 | `.../node_modules bucket includes resolved packages from all projects` | — |
| `registry::symlink_monorepo_invalidate` | 符号链接 monorepo 源文件改动触发失效 | `.../symlinked monorepo invalidates on source file change` | — |
| `registry::pnpm_granular_updates` | pnpm 符号链接仅给 workspace 包细粒度更新 | `.../pnpm-style symlinks only grant granular updates to workspace packages` | — |
| `registry::file_exclude_rebuild` | fileExcludePatterns 变更触发重建 | `.../changed fileExcludePatterns triggers bucket rebuild` | — |
| `registry::dedupe_realpath` | 跨祖先 node_modules 桶去重 realpath 相同的包 | `.../dedupes packages that resolve to same realpath across ancestor node_modules buckets` | — |

### TestHiddenDirectoriesInNodeModules（1 子用例）
| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `hidden_dirs::deep_import_subdir_pkgjson` | 隐藏 store 中经子目录 package.json 的深导入 | `TestHiddenDirectoriesInNodeModules/deep import through subdirectory package.json in hidden store` | — |

### TestAutoImportEntrypointDirectorySearch（4 子用例）
| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `entrypoint::default_main_only` | 默认仅限主入口 | `.../default limits to main entrypoint` | — |
| `entrypoint::enable_all_files` | `autoImportEntrypointDirectorySearch` 启用全文件 | `.../autoImportEntrypointDirectorySearch enables all files` | — |
| `entrypoint::pref_change_rebuild` | 改偏好触发重建 | `.../changing preference triggers rebuild` | — |
| `entrypoint::deep_import_recursive` | 程序更新里的深导入启用该包递归搜索 | `.../deep import from program update enables recursive search for that package` | — |

## `autoimport/testmain_test.go`
> `func TestMain`（`core.ApplyDebugStackLimit()` + `baseline.Track()`）。Rust 无 `TestMain`；用 `#[cfg(test)]` 全局 setup（或 `ctor`/测试 harness）等价处理 baseline 钩子。**非行为用例**，不计入对齐 gate。

---

## `format_test.go`（ls 根，非 lsutil）

> 3 个 func，均用 `&LanguageService{}`（零值，**无 program/host**）直接调内部 `getFormattingEditsAfterKeystroke`/`getFormattingEditsForRange`，核心断言是「**不 panic**」（回归：空文件回车、范围越出函数体）。

### TestGetFormattingEditsAfterKeystroke_EmptyFile（1 用例）
| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `keystroke::empty_file_enter` | 空文件 pos=0 回车不 panic | `text=""`,pos=0,key="\n" → 不 panic（返回 nil/空） | `TestGetFormattingEditsAfterKeystroke_EmptyFile` | |

### TestGetFormattingEditsAfterKeystroke_SimpleStatement（1 用例）
| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `keystroke::simple_statement_enter` | `"const x = 1"` 末尾回车不 panic | pos=len,key="\n" → 不 panic | `TestGetFormattingEditsAfterKeystroke_SimpleStatement` | |

### TestGetFormattingEditsForRange_FunctionBody（表驱动，4 子用例）
| Rust 测试 | 验证内容 | input(text, start, end) → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `range::return_in_function` | 函数内 return 范围格式化不 panic | `"function foo() {\n    return (1  + 2);\n}"`,21,38 → 不 panic | `.../return statement in function` | |
| `range::newline_after_keyword` | function 后换行 | `"function\nf() {\n}"`,9,13 → 不 panic | `.../function with newline after keyword` | |
| `range::empty_body` | 空函数体 | `"function f() {\n  \n}"`,15,17 → 不 panic | `.../empty function body` | |
| `range::after_closing_brace` | 闭合括号后 | `"function f() {\n}"`,15,15 → 不 panic | `.../after function closing brace` | |

---

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（21，含 TestMain）都已映射。
- [x] 每个表驱动子用例都已逐行列出（converters 19+18+14、userpreferences 2+4+9+4、utilities 3、index 4、wordIndices 13、realpath 3、registry 11+1+4、format 1+1+4）。
- [x] expected 值均取自 Go 测试字面量（URI/数值/布尔/词切片）。
- [x] 每条带 `// Go:` 锚点（「Go 对照」列）。
- [x] 双向对齐：被测函数 `compute_lsp_line_starts`/`line_and_character_to_position`/`position_to_line_and_character`/`file_name_to_document_uri`/`with_config`/`marshal/unmarshal`/`parse_user_preferences`/`probably_uses_semicolons`/`Index::*`/`word_indices`/`get_package_realpath_funcs`/`Registry::*`/`get_formatting_edits_*` 均在 impl.md 有承载 TODO。

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| **补全**（符号/关键字/JSX/auto-import/snippet/string literal/module path） | Go 侧无直接单测 | P10 fourslash（`tests/cases/fourslash/completion*`） |
| **悬停 / quickInfo** | 同上 | P10 fourslash（`*quickInfo*`） |
| **查找引用 / 实现 / 文档高亮** | 同上 | P10 fourslash（`*findAllRef*`/`*reference*`/`*documentHighlights*`） |
| **重命名 / 文件重命名** | 同上 | P10 fourslash（`*rename*`） |
| **签名帮助** | 同上 | P10 fourslash（`*signature*`） |
| **跳转定义 / 类型定义 / source definition** | 同上 | P10 fourslash（`*goto*`/`*definition*`） |
| **语义高亮 / inlay hints / 折叠 / 文档&工作区符号 / 选区 / linked editing / autoinsert / codelens** | 同上 | P10 fourslash + baseline |
| **code action / quick fix（import/isolatedDeclarations/implements interface）/ organize imports** | 同上 | P10 fourslash（`*codeFix*`/`*organizeImports*`/`*importFixes*`） |
| **文档诊断（ProvideDiagnostics）** | 依赖 program/checker | P10 conformance/fourslash |
| **调用层级（call hierarchy）** | 同上 | P10 fourslash（`*callHierarchy*`） |
| **跨工程（crossproject）合并/编排** | 依赖 P8 project 编排实现 | P8（project 集成测）+ P10 |
| auto-import **registry** 细粒度补全输出 | 单测只测桶生命周期；实际补全内容 | P10 fourslash（`*autoImport*`） |
| `DocumentUri.FileName()`（TestDocumentURIToFileName） | 实现归 `lsproto` | P8（`tsgo_lsp_lsproto`） |
| `realpath::*` / `registry::*`（用 vfstest） | 依赖 `vfstest` 测试设施 | 可在 P7 跑（vfstest 属 P1）；标 DEFER 仅指接线就绪 |
