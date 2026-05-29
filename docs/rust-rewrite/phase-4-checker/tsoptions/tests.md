# tsoptions: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：8 测试文件 / 17 `func Test*`（+`TestMain`+2 Benchmark）/ 共约 100+ 子用例。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/tsoptions/tsconfigparsing_test.go` | `tsconfigparsing.rs`（`#[cfg(test)] mod tests`）/ `tests/tsconfigparsing.rs` | 6（+1 Benchmark） |
| `internal/tsoptions/commandlineparser_test.go` | `commandlineparser.rs` / `tests/commandlineparser.rs` | 7 |
| `internal/tsoptions/wildcarddirectories_test.go` | `wildcarddirectories.rs`（包内测试） | 1 |
| `internal/tsoptions/parsinghelpers_test.go` | `parsinghelpers.rs`（包内测试） | 1 |
| `internal/tsoptions/parsedcommandline_test.go` | `tests/parsedcommandline.rs` | 1 |
| `internal/tsoptions/decls_test.go` | `tests/decls.rs` | 1 |
| `internal/tsoptions/testmain_test.go` | （`TestMain` 等价：测试 harness，baseline.Track；Rust 不需要） | —(harness) |
| `internal/tsoptions/export_test.go` | 测试可见性帮手（`ParseCommandLineTestWorker`/`TestCommandLineParser`），非 Test | —(helper) |

> **baseline 说明**：很多 Go 用例走 `baseline.Run`/`baseline.RunAgainstSubmodule`（golden 文件对拍）。本 phase 的 Rust gate 用**断言级**覆盖每个子用例的关键产物（fileNames / compilerOptions 字段 / errors 数量与 code）；与 Go 的 golden 字节级对拍统一归 **P10 parity**。下表的每个子用例都逐行列出（这是 1:1 复原的核心），完成列对断言级可勾，对 golden 字节级标 `—`(P10)。

---

## `tsconfigparsing_test.go`

### TestParseConfigFileTextToJson（表 `parseConfigFileTextToJsonTests`，submodule baseline）

`ParseConfigFileTextToJson("/apath/tsconfig.json","/apath", jsonText)` → 解析后的 JSON 值 + errors，写 baseline。每个标题含多个 input 字符串。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `text_to_json_only_whitespace` | 纯空白 → 空配置无错 | `""` / `" "` → 空对象, 0 errors | `...:TestParseConfigFileTextToJson/returns empty config for file with only whitespaces` | — (P10 golden) |
| `text_to_json_comments_only` | 仅注释 → 空配置 | `"// Comment"` / `"/* Comment*/"` → 空对象 | `.../returns empty config for file with comments only` | — |
| `text_to_json_empty_object` | `{}` → 空配置 | `{}` → 空对象 | `.../returns empty config when config is empty object` | — |
| `text_to_json_strips_comments` | 带行/块注释的 exclude 数组 | 含 `// Exclude` 注释 → `exclude:["file.d.ts"]` | `.../returns config object without comments` | — |
| `text_to_json_keeps_string_content` | 字符串内 `//`、`/*` 不当注释 | `"xx//file.d.ts"` / `"xx/*file.d.ts*/"` 原样保留 | `.../keeps string content untouched` | — |
| `text_to_json_escaped_chars` | 字符串转义正确处理 | `"xx\"//files"` / `"xx\\"` 行尾注释 | `.../handles escaped characters in strings correctly` | — |
| `text_to_json_lib_array` | lib 数组正确解析 | `lib:["es5"]` / `lib:["es5","es6"]` | `.../returns object when users correctly specify library` | — |

### TestParseJsonConfigFileContent / TestParseJsonSourceFileConfigFileContent（表 `parseJsonConfigFileTests`）

两个 Test 用同一张表，分别走 `getParsedWithJsonApi`（`ParseJsonConfigFileContent`）与 `getParsedWithJsonSourceFileApi`（`ParseJsonSourceFileConfigFileContent`）。断言关键产物：`ParsedConfig.CompilerOptions`、`FileNames`、`Errors`。Rust 侧每个子用例各跑两 API。

| Rust 测试（×2 api） | 验证内容 | input → expected（关键断言） | Go 对照 | 完成 |
|---|---|---|---|---|
| `cfg_ignore_dotted` | 忽略点开头文件/目录 | `{}` + 含 `.git/`/`.b.ts`/`..c.ts` → fileNames 仅 `test.ts` | `parseJsonConfigFileTests/ignore dotted files and folders` | — |
| `cfg_allow_dotted_explicit` | `files` 显式列出点文件被收 | `files:[.git/a.ts,.b.ts,..c.ts]` → 这三个入列 | `.../allow dotted files and folders when explicitly requested` | — |
| `cfg_exclude_pkg_folders` | 隐式排除 node_modules/bower/jspm | `{}` → 仅 `/d.ts`,`/folder/e.ts` | `.../implicitly exclude common package folders` | — |
| `cfg_empty_files_err` | 空 `files` 报错 | `files:[]` → 含 `The_files_list_in_config_file_0_is_empty` | `.../generates errors for empty files list` | — |
| `cfg_empty_files_no_refs_err` | 空 files + 空 references 报错 | `files:[],references:[]` → 同上错误 | `.../generates errors for empty files list when no references are provided` | — |
| `cfg_dir_no_ts_err` | 目录无 .ts | `{}` + 仅 `a.js` → No_inputs_were_found | `.../generates errors for directory with no .ts files` | — |
| `cfg_empty_include_err` | 空 include | `include:[]` → No_inputs_were_found | `.../generates errors for empty include` | — |
| `cfg_full_options` | 完整选项解析（noSubmoduleBaseline） | outDir/strict/target=ES2017/module=ESNext/moduleResolution=bundler/jsx=react/maxNodeModuleJsDepth=1/paths/files/include/exclude → 对应字段 + fileNames | `.../parses tsconfig with compilerOptions, files, include, and exclude` | |
| `cfg_cmdline_only_in_tsconfig_err` | tsconfig 里写命令行专属选项 | `help:true` → Option_0_can_only_be_specified_on_command_line | `.../generates errors when commandline option is in tsconfig` | |
| `cfg_empty_files_with_refs_ok` | 空 files + 有 references 不报错 | `files:[],references:[{path:/apath}]` → 0 该类 error | `.../does not generate errors for empty files list when one or more references are provided` | |
| `cfg_exclude_outdir` | 默认排除 outDir（2 inputs） | outDir=bin → 排除 `/bin/a.ts`；exclude=[obj] 覆盖 → 不排除 bin | `.../exclude outDir unless overridden` | — |
| `cfg_exclude_decldir` | 默认排除 declarationDir（2 inputs） | declarationDir=declarations → 排除；exclude=[types] 覆盖 | `.../exclude declarationDir unless overridden` | — |
| `cfg_empty_dir_err` | 空目录 | allowJs:true + 无文件 → No_inputs_were_found | `.../generates errors for empty directory` | |
| `cfg_includes_with_outdir` | include `**/*` + outDir=./ | → 解析（含/不含 outDir 文件行为） | `.../generates errors for includes with outDir` | — |
| `cfg_include_not_string_err` | include 元素非字符串 | `include:[["./**/*.ts"]]` → Compiler_option_0_requires_a_value_of_type_1 | `.../generates errors when include is not string` | |
| `cfg_files_not_string_err` | files 元素非字符串 | `files:[{compilerOptions:...}]` → 类型错误 | `.../generates errors when files is not string` | |
| `cfg_outdir_from_base` | 从 base 继承 outDir（2 inputs，configDir） | extends tsconfigWithoutConfigDir / tsconfigWithConfigDir → outDir 继承/`${configDir}` 替换 | `.../with outDir from base tsconfig` | — |
| `cfg_excludes_typo_err` | `excludes`（拼写）报错 | `excludes:[foge.ts]` → Unknown_option_excludes_Did_you_mean_exclude | `.../returns error when tsconfig have excludes` | — |
| `cfg_extends_options` | extends + 本地选项合并（noSubmoduleBaseline） | extends tsconfigWithExtends + outDir/strict/noImplicitAny/baseUrl → 合并后字段 + 继承 files/include | `.../parses tsconfig with extends, files, include and other options` | |
| `cfg_extends_configdir` | extends + `${configDir}`（noSubmoduleBaseline） | extends tsconfig.base（含 6 个 `${configDir}/...`）→ 全部替换为绝对路径 | `.../parses tsconfig with extends and configDir` | |
| `cfg_unknown_option_err` | 未知 compiler 选项 | `unknown:true` → Unknown_compiler_option_0 | `.../reports error for an unknown option` | |
| `cfg_wrong_type_and_enum_err` | 类型错 + 无效枚举 | target="invalid"/removeComments="should be boolean"/moduleResolution="invalid" → 各诊断 | `.../reports errors for wrong type option and invalid enum value` | |
| `cfg_wrong_case_options` | 大小写错（noSubmoduleBaseline） | sourcemap/declarationmap/... → did-you-mean 大小写诊断 | `.../reports errors for incorrectly cased option names` | |
| `cfg_empty_types_array` | 空 types 数组（noSubmoduleBaseline） | `types:[]` → `Types==[]`（保留空数组） | `.../handles empty types array` | |
| `cfg_issue_1267_extended_files` | extends 的 files 被拾起（noSubmoduleBaseline） | extends backend.json（含 files 列表 + 相对路径重写）→ types/*.d.ts 入列 | `.../issue 1267 scenario - extended files not picked up` | |
| `cfg_null_override_arrays` | null 覆盖数组字段（noSubmoduleBaseline） | extends base; types/lib/typeRoots=null → 三者清空 | `.../null overrides in extended tsconfig - array fields` | |
| `cfg_null_override_strings` | null 覆盖字符串字段（noSubmoduleBaseline） | outDir/baseUrl/rootDir=null → 清空 | `.../null overrides in extended tsconfig - string fields` | |
| `cfg_null_override_mixed` | null 覆盖混合（noSubmoduleBaseline） | types/outDir/allowJs=null + strict=false + lib=[es2022] → 混合结果 | `.../null overrides in extended tsconfig - mixed field types` | |
| `cfg_null_override_multi_extends` | 多层 extends + null（noSubmoduleBaseline） | middle←base; types/lib=null → 清空, 保留 middle.outDir | `.../null overrides with multiple extends levels` | |
| `cfg_null_override_middle_level` | 中间层 null（noSubmoduleBaseline） | middle 设 types/lib=null; final 设 outDir → final.outDir 生效, types/lib 空 | `.../null overrides in middle level of extends chain` | |

### TestParseTypeAcquisition（cases 表，本地 baseline，不需 submodule）

每 case 跑 `with json api` + `with jsonSourceFile api`，断言 `ParsedConfig.TypeAcquisition`。

| Rust 测试（×2 api） | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `ta_correct_format` | 正确 typeAcquisition | enable:true,include:[0.d.ts,1.d.ts],exclude:[0.js,1.js] → 对应字段 | `TestParseTypeAcquisition/Convert correctly format tsconfig.json...` | |
| `ta_incorrect_format` | 错误键 `enableAutoDiscovy` | → 忽略未知键，enable 未设 | `.../Convert incorrect format tsconfig.json...` | |
| `ta_default_tsconfig` | 默认 `{}` (tsconfig) | → enable 未启用 | `.../Convert default tsconfig.json...` | |
| `ta_only_enable` | 仅 enable | enable:true → enable=true | `.../Convert tsconfig.json with only enable property...` | |
| `ta_jsconfig` | jsconfig.json | enable:false,include:[0.d.ts],exclude:[0.js] | `.../Convert jsconfig.json to typeAcquisition` | |
| `ta_default_jsconfig` | 默认 jsconfig | `{}` → **enable 默认 true**（jsconfig 特例） | `.../Convert default jsconfig.json...` | |
| `ta_jsconfig_incorrect` | jsconfig 错误键 | `enableAutoDiscovy` → 默认 enable=true | `.../Convert incorrect format jsconfig.json...` | |
| `ta_jsconfig_only_enable` | jsconfig 仅 enable:false | → enable=false | `.../Convert jsconfig.json with only enable property...` | |

### TestParseSrcCompiler（需 submodule，直接断言）

解析 TS submodule 的 `src/compiler/tsconfig.json`，`assert.DeepEqual` 比较 `CompilerOptions`（lib/module=NodeNext/moduleResolution/newLine=LF/outDir/target=ES2020/types=[node]/Declaration/.../Pretty 等十余字段）与 fileNames 相对路径列表（binder.ts...module/system.ts，~70 文件）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parse_src_compiler_options` | 解析真实 compiler tsconfig 的选项 | → 字段与 Go 字面量逐一相等（`cmpopts.IgnoreUnexported`） | `TestParseSrcCompiler` | — (P10, 需 submodule) |
| `parse_src_compiler_filenames` | 解析出的文件相对路径列表 | → 与 Go 列出的 ~70 个相对路径相等 | `TestParseSrcCompiler` | — (P10) |

### TestExtendedConfigErrorsAppearOnCacheHit（2 子测试，直接断言）

用 `memoCache`（最小 memoizing `ExtendedConfigCache`）验证：extends 的诊断在 cache 命中时仍上报。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `extended_cache_single_config_twice` | 同 config 解析两次都报错 | base.json 含 `excludes`（拼写）→ 第一次 & 第二次（cache hit）`Errors>0` | `TestExtendedConfigErrorsAppearOnCacheHit/single config parsed twice` | |
| `extended_cache_two_share_base` | 两 config 共享同 base | projA/projB 都 extends base（含 excludes）→ 两次均 `Errors>0` | `.../two configs share same base` | |

---

## `commandlineparser_test.go`

### TestCommandLineParseResult（表 `parseCommandLineSubScenarios`，submodule baseline）

`ParseCommandLineTestWorker(decls, args, fs, tmp)` → 比对 baseline 的 `fileNames`/`options`/`errors`。每条逐子用例列：

| Rust 测试 | 验证内容（args） | Go 对照（子用例名） | 完成 |
|---|---|---|---|
| `cl_lib_single` | `[--lib es6 0.ts]` | `Parse single option of library flag` | — (P10 golden) |
| `cl_build_flags_combo` | `[--build --clean --dry --force --verbose]` | `Handles may only be used with --build flags` | — |
| `cl_did_you_mean` | `[--declarations --allowTS]` | `Handles did you mean for misspelt flags` | — |
| `cl_lib_multi` | `[--lib es5,es2015.symbol.wellknown 0.ts]` | `Parse multiple options of library flags` | — |
| `cl_lib_invalid` | `[--lib es5,invalidOption 0.ts]` | `Parse invalid option of library flags` | — |
| `cl_empty_jsx` | `[0.ts --jsx]` | `Parse empty options of --jsx` | — |
| `cl_empty_module` | `[0.ts --module]` | `Parse empty options of --module` | — |
| `cl_empty_newline` | `[0.ts --newLine]` | `Parse empty options of --newLine` | — |
| `cl_empty_target` | `[0.ts --target]` | `Parse empty options of --target` | — |
| `cl_empty_moduleresolution` | `[0.ts --moduleResolution]` | `Parse empty options of --moduleResolution` | — |
| `cl_empty_lib` | `[0.ts --lib]` | `Parse empty options of --lib` | — |
| `cl_empty_string_lib` | `[0.ts --lib ""]`（空串 falsey） | `Parse empty string of --lib` | — |
| `cl_lib_followed_by_option` | `[0.ts --lib --sourcemap]` | `Parse immediately following command line argument of --lib` | — |
| `cl_lib_extra_comma` | `[--lib es5, es7 0.ts]` | `Parse --lib option with extra comma` | — |
| `cl_lib_trailing_ws` | `[--lib "es5, " es7 0.ts]` | `Parse --lib option with trailing white-space` | — |
| `cl_multi_flags_files_end` | `[--lib ... --target es5 0.ts]` | `Parse multiple compiler flags with input files at the end` | — |
| `cl_multi_flags_files_mid` | `[--module commonjs --target es5 0.ts --lib ...]` | `Parse multiple compiler flags with input files in the middle` | — |
| `cl_multi_lib_flags` | 多个 `--lib` | `Parse multiple library compiler flags ` | — |
| `cl_bool_explicit_false` | `[--strictNullChecks false 0.ts]` | `Parse explicit boolean flag value` | — |
| `cl_bool_nonbool_after` | `[--noImplicitAny t 0.ts]`（t 不消费） | `Parse non boolean argument after boolean flag` | — |
| `cl_bool_implicit` | `[--strictNullChecks]` | `Parse implicit boolean flag value` | — |
| `cl_incremental` | `[--incremental 0.ts]` | `parse --incremental` | — |
| `cl_tsbuildinfofile` | `[--tsBuildInfoFile build.tsbuildinfo 0.ts]` | `parse --tsBuildInfoFile` | — |
| `cl_tsconfig_only_null` | `[--composite null -tsBuildInfoFile null 0.ts]` | `allows tsconfig only option to be set to null` | — |
| `cl_watchfile` | `[--watchFile UseFsEvents 0.ts]` | `parse --watchFile` | — |
| `cl_watchdirectory` | `[--watchDirectory FixedPollingInterval 0.ts]` | `parse --watchDirectory` | — |
| `cl_fallbackpolling` | `[--fallbackPolling PriorityInterval 0.ts]` | `parse --fallbackPolling` | — |
| `cl_sync_watchdir` | `[--synchronousWatchDirectory 0.ts]` | `parse --synchronousWatchDirectory` | — |
| `cl_fallbackpolling_missing` | `[0.ts --fallbackPolling]`（缺参） | `errors on missing argument to --fallbackPolling` | — |
| `cl_excludedirs` | `[--excludeDirectories **/temp 0.ts]` | `parse --excludeDirectories` | — |
| `cl_excludedirs_invalid` | `[--excludeDirectories **/../* 0.ts]` | `errors on invalid excludeDirectories` | — |
| `cl_excludefiles` | `[--excludeFiles **/temp/*.ts 0.ts]` | `parse --excludeFiles` | — |
| `cl_excludefiles_invalid` | `[--excludeFiles **/../* 0.ts]` | `errors on invalid excludeFiles` | — |

### TestResponseFileDoesNotPanic（2 子测试，直接断言）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `response_file_empty` | `@`（空文件名）不 panic | `[@]` → `Errors>0`（无 panic） | `TestResponseFileDoesNotPanic/empty response file` | |
| `response_file_relative_missing` | `@blah`（不存在）不 panic | `[@blah]` → `Errors>0` | `.../relative response file` | |

### TestParseCommandLineTypeRootsRelativePath（直接断言）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `typeroots_relative_to_absolute` | typeRoots 相对路径转绝对 | `[--typeRoots t bug.ts]`（cwd=/home/project）→ typeRoots 长度 1、绝对路径、以 `/t` 结尾 | `TestParseCommandLineTypeRootsRelativePath` | |

### TestCustomConditionsNullOverride（直接断言）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `custom_conditions_null_override` | `--customConditions null` 覆盖 tsconfig | raw 中 customConditions=nil；经 `GetParsedCommandLineOfConfigFile` 合并后 `CustomConditions==nil`，0 errors | `TestCustomConditionsNullOverride` | |

### TestParseCommandLineVerifyNull（submodule baseline + verifyNull 表）

包含 1 个直接 boolean 子用例 + 对每个 `verifyNull` 生成 3~4 个子场景（allows null / errors if non-null / errors if followed by option / errors if last option）。

| Rust 测试 | 验证内容（args） | Go 对照 | 完成 |
|---|---|---|---|
| `vn_bool_false` | `[--composite false 0.ts]` | `allows setting option type boolean to false` | — (P10 golden) |
| `vn_boolean_null` | composite null | `option of type boolean allows setting it to null` | — |
| `vn_boolean_nonnull_err` | composite true | `option of type boolean errors if non null value is passed` | — |
| `vn_boolean_followed_err` | `[0.ts --strictNullChecks --composite]` | `option of type boolean errors if its followed by another option` | — |
| `vn_boolean_last_err` | `[0.ts --composite]` | `option of type boolean errors if its last option` | — |
| `vn_object_*` | paths null / followed / last（无 nonNullValue） | `option of type object ...`（3 子场景） | — |
| `vn_list_*` | rootDirs null / nonnull(abc,xyz) / followed / last | `option of type list ...`（4 子场景） | — |
| `vn_string_*` | string null / nonnull(hello) / followed / last | `option of type string ...`（4 子场景） | — |
| `vn_number_*` | number null / nonnull(10) / followed / last | `option of type number ...`（4 子场景） | — |

### TestParseBuildCommandLine（表，submodule baseline + extraScenarios 无 baseline）

`ParseBuildCommandLine` → 断言 `Projects`/`BuildOptions`/`CompilerOptions`/`Errors`。前 21 条走 submodule baseline，后 5 条（extraScenarios）`getTsBaseline=nil` 仅写本地 baseline。

| Rust 测试 | 验证内容（args） | Go 对照 | 完成 |
|---|---|---|---|
| `bcl_no_options` | `[]`（默认 projects=`.`） | `parse build without any options ` | — (P10) |
| `bcl_multi_options` | `[--verbose --force tests]` | `Parse multiple options` | — |
| `bcl_invalid_option` | `[--verbose --invalidOption]` | `Parse option with invalid option` | — |
| `bcl_flags_projects_end` | `[--force --verbose src tests]` | `Parse multiple flags with input projects at the end` | — |
| `bcl_flags_projects_mid` | `[--force src tests --verbose]` | `Parse multiple flags with input projects in the middle` | — |
| `bcl_flags_projects_begin` | `[src tests --force --verbose]` | `Parse multiple flags with input projects in the beginning` | — |
| `bcl_incremental` | `[--incremental tests]` | `parse build with --incremental` | — |
| `bcl_locale` | `[--locale en-us src]` | `parse build with --locale en-us` | — |
| `bcl_tsbuildinfofile` | `[--tsBuildInfoFile build.tsbuildinfo tests]` | `parse build with --tsBuildInfoFile` | — |
| `bcl_common_not_with_build` | `[--strict]` | `reports other common may not be used with --build flags` | — |
| `bcl_clean_force_invalid` | `[--clean --force]` | `--clean and --force together is invalid` | — |
| `bcl_clean_verbose_invalid` | `[--clean --verbose]` | `--clean and --verbose together is invalid` | — |
| `bcl_clean_watch_invalid` | `[--clean --watch]` | `--clean and --watch together is invalid` | — |
| `bcl_watch_dry_invalid` | `[--watch --dry]` | `--watch and --dry together is invalid` | — |
| `bcl_watchfile` | `[--watchFile UseFsEvents --verbose]` | `parse --watchFile` | — |
| `bcl_watchdirectory` | `[--watchDirectory FixedPollingInterval --verbose]` | `parse --watchDirectory` | — |
| `bcl_fallbackpolling` | `[--fallbackPolling PriorityInterval --verbose]` | `parse --fallbackPolling` | — |
| `bcl_sync_watchdir` | `[--synchronousWatchDirectory --verbose]` | `parse --synchronousWatchDirectory` | — |
| `bcl_missing_arg` | `[--verbose --fallbackPolling]` | `errors on missing argument` | — |
| `bcl_excludedirs_invalid` | `[--excludeDirectories **/../*]` | `errors on invalid excludeDirectories` | — |
| `bcl_excludefiles` | `[--excludeFiles **/temp/*.ts]` | `parse --excludeFiles` | — |
| `bcl_excludefiles_invalid` | `[--excludeFiles **/../*]` | `errors on invalid excludeFiles` | — |
| `bcl_builders` | `[--builders 2]` | `parse --builders`（extraScenario，本地） | |
| `bcl_singlethreaded_builders` | `[--singleThreaded --builders 2]` | `--singleThreaded and --builders together` | |
| `bcl_builders_zero_err` | `[--builders 0]`（minValue=1） | `reports error when --builders is 0` | |
| `bcl_builders_negative_err` | `[--builders -1]` | `reports error when --builders is negative` | |
| `bcl_builders_invalid_type_err` | `[--builders invalid]` | `reports error when --builders is invalid type` | |

### TestAffectsBuildInfo（直接断言）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `affects_buildinfo_superset_of_semantic` | 所有 affectsSemanticDiagnostics 的选项必 affectsBuildInfo | 遍历 `OptionsDeclarations`：`AffectsSemanticDiagnostics ⇒ AffectsBuildInfo` | `TestAffectsBuildInfo` | |

---

## `wildcarddirectories_test.go`

### TestGetWildcardDirectories_NonASCIICharacters（表，4 子用例，直接断言）

`getWildcardDirectories(include, exclude, cpo)` 仅断言 `result != nil`（非 ASCII 路径不崩）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `wildcard_norwegian` | 挪威字符 æ | cwd 含 `TobiasLægreid` → 非 nil | `.../Norwegian character æ in path` | |
| `wildcard_japanese` | 日文字符 | cwd `/Users/ユーザー/プロジェクト`, exclude `テスト` → 非 nil | `.../Japanese characters in path` | |
| `wildcard_chinese` | 中文字符 | cwd `/home/用户/项目`, include `源代码/**/*.js` → 非 nil | `.../Chinese characters in path` | |
| `wildcard_various_unicode` | 多种 Unicode | cwd `/Users/Müller/café/naïve/résumé` → 非 nil | `.../Various Unicode characters` | |

> 补充（impl 行为级，超出 Go 断言）：可加 `recursive vs non-recursive` 判定用例（`/a/b/**/d` 递归、`/a/b/*` 非递归），expected 取自 `getWildcardDirectoryFromSpec` 注释里的语义说明。

---

## `parsinghelpers_test.go`

### TestParseCompilerOptionNoMissingFields（一致性 gate，直接断言）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parse_compiler_option_no_missing_fields` | `CompilerOptions` 每个导出字段都被 `parse_compiler_options` switch 覆盖 | 遍历字段（用 json tag 名）→ `parse_compiler_options(key, zero, &mut co)` 返回 true | `TestParseCompilerOptionNoMissingFields` | |

> **反射替代 gate**：Rust 不能用 reflect，需用字段表/宏枚举字段名。此测试是"巨型 switch 完整性"的护栏，必须保留。

---

## `parsedcommandline_test.go`

### TestParsedCommandLine / PossiblyMatchesFileName（嵌套子测试，直接断言）

用 `tsoptionstest.GetParsedCommandLine` 构造，`assertMatches` 逐文件比对 `PossiblyMatchesFileName`。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `pcl_literal_files_no_exclude` | files 列表匹配 | `files:[a.ts,b.ts]` → 仅 `/dev/a.ts`,`/dev/b.ts` 匹配 | `TestParsedCommandLine/PossiblyMatchesFileName/with literal file list/without exclude` | |
| `pcl_literal_files_not_removed_by_exclude` | exclude 不移除 files 列出项 | files:[a,b] + exclude:[b.ts] → a,b 仍匹配；reload(空FS) 后仍匹配 | `.../with literal file list/are not removed due to excludes` | |
| `pcl_literal_files_dedup` | files 去重 | files:[a,a,b] → `LiteralFileNames()==[a,b]` | `.../with literal file list/duplicates` | |
| `pcl_literal_include_no_exclude` | include 列表匹配 | include:[a.ts,b.ts] → a,b 匹配；reload(空FS) 后仍匹配 | `.../with literal include list/without exclude` | |

---

## `decls_test.go`

### TestCompilerOptionsDeclaration（一致性 gate，直接断言）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `compiler_options_declaration_bijection` | 声明 ↔ CompilerOptions 字段双向覆盖 | 每个声明有对应字段、每个字段有声明（除 internalOptions 7 项 + skipped `plugins`）；json tag == `name+",omitzero"` | `TestCompilerOptionsDeclaration` | |

> internalOptions：`allowNonTsExtensions`/`build`/`configFilePath`/`noDtsResolution`/`noEmitForJsFiles`/`pathsBasePath`/`suppressOutputPathCheck`。skipped：`plugins`。

---

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（17 个）都已映射（TestMain/export_test 为 harness/helper，已说明）
- [x] 每个表驱动子用例都已逐行列出（`parseConfigFileTextToJsonTests` 7 / `parseJsonConfigFileTests` 30 / `TestParseTypeAcquisition` 8×2 / `parseCommandLineSubScenarios` 33 / build 27 / verifyNull 多场景 / wildcard 4 / PossiblyMatchesFileName 4）
- [x] expected 值均取自 Go 测试字面量/断言（fileNames、字段值、错误 code、`/t` 后缀等）
- [x] 每条带 `// Go:` 锚点（"Go 对照"列）
- [x] 与 impl.md 双向对齐：涉及的 `parse_command_line`/`parse_build_command_line`/`parse_json_*`/`get_wildcard_directories`/`parse_compiler_options`/`possibly_matches_file_name`/`parse_extended_config`/`affects_*` 均有实现 TODO

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 所有 `baseline.Run*` golden 字节级对拍 | baseline 框架 + golden 文件 | P10 parity（本 phase 用断言级覆盖关键字段） |
| `TestParseSrcCompiler`（真实 src/compiler tsconfig） | 需 TS submodule + module/glob/vfsmatch 全实现 | P10 parity |
| 需 submodule 的 `TestCommandLineParseResult`/`TestParseBuildCommandLine`/`TestParseConfigFileTextToJson`/`TestParseJsonConfigFileContent` golden | submodule baseline | P10 parity |
| 通配文件展开与 Go 完全一致（扩展名优先级/json/casing） | 需 `vfsmatch.ReadDirectory`（P1）落地 | P1 / P10 |
| `extends` 走 module 解析（非相对路径 base） | 需 `module.ResolveConfig`（P4） | P4 / P10 |
