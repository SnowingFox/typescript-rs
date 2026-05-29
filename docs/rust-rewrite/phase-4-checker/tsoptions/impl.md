# tsoptions: 实现方案（impl.md）

**crate**：`tsgo_tsoptions`　**目标**：解析与校验编译器选项——命令行参数（`tsc` / `tsc -b`）、`tsconfig.json`/`jsconfig.json`（含 `extends` 继承、`${configDir}` 替换、`files`/`include`/`exclude` 通配展开、project references），并提供选项声明表（option declarations）与各种名字映射 / 枚举映射。
**依赖（crate）**：`tsgo_ast` `tsgo_collections` `tsgo_core` `tsgo_diagnostics` `tsgo_glob` `tsgo_jsnum` `tsgo_locale` `tsgo_module` `tsgo_modulespecifiers` `tsgo_outputpaths` `tsgo_parser` `tsgo_scanner` `tsgo_stringutil` `tsgo_tspath` `tsgo_vfs`（含 `vfs/vfsmatch`）`tsgo_debug`。镜像 Go import 边。
**Go 源**：`internal/tsoptions/`（16 个非测试 `.go` + 子目录 `tsoptionstest/` 2 个 = 18 文件；最大 `tsconfigparsing.go` 1815 行 + `declscompiler.go` 1265 行）

## 这个包是什么（业务说明）

`tsoptions` 是编译器的"配置前端"。它把两类外部输入翻译成结构化的 `core.CompilerOptions` / `WatchOptions` / `BuildOptions` / `TypeAcquisition` + 文件名列表 + project references：

1. **命令行**（`commandlineparser.go`）：`ParseCommandLine`（`tsc ...`）/`ParseBuildCommandLine`（`tsc -b ...`）逐 token 扫描，遇 `-x`/`--x` 查选项声明、按类型（string/number/boolean/list/enum）消费值，遇 `@file` 读响应文件，其余当文件名。
2. **配置文件**（`tsconfigparsing.go`）：`ParseJsonSourceFileConfigFileContent`/`ParseJsonConfigFileContent` 把 tsconfig 的 JSON AST（来自 `parser`）转成选项对象——核心难点是 `extends` 链合并（`mergeCompilerOptions` + 显式 `null` 覆盖）、`${configDir}` 模板替换、`files`/`include`/`exclude` 规格校验、通配目录展开成实际文件列表（`getFileNamesFromConfigSpecs` 走 `vfsmatch.ReadDirectory`，含扩展名优先级去重）、project references 解析。

支撑这两条主路径的是大量"数据 + 映射"文件：`declscompiler.go`/`declsbuild.go`/`declswatch.go`/`declstypeacquisition.go`（选项声明表，纯数据）、`enummaps.go`（lib/target/module/jsx/... 的字符串→枚举映射）、`namemap.go`（名字→声明、短名映射）、`commandlineoption.go`（`CommandLineOption` 类型 + 关联映射）、`parsinghelpers.go`（值解析器 + `ParseCompilerOptions` 巨型 switch）、`diagnostics.go`/`errors.go`（"did you mean" / 未知选项 / 类型不匹配诊断）、`wildcarddirectories.go`（监视目录推导）、`parsedcommandline.go`/`parsedbuildcommandline.go`（解析结果 + 大量惰性缓存）、`showconfig.go`（`--showConfig` 反向序列化 + implied options）。

它处在 Phase 6 是因为依赖巨多（`module`/`parser`/`outputpaths`/`glob`/`vfsmatch` 都得先有），且它是 `compiler`（Program 构建）的直接上游——Program 拿到的是 `ParsedCommandLine`。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `CompilerOptionsValue = any` / 选项值 `any` | `enum OptionValue { Null, Bool(bool), Int(i64), Float(f64), Str(String), List(Vec<OptionValue>), Map(IndexMap<String,OptionValue>), Enum(...) }` | tsconfig JSON 解析中间表示。**核心偏离**：Go 全程 `any`；Rust 用判别联合。见偏离 |
| `*collections.OrderedMap[string, any]`（解析中保序） | `IndexMap<String, OptionValue>` | **保序是硬要求**：影响 emit/诊断/`raw` 输出顺序。对应 `parser.options`、tsconfig 的 raw |
| `CommandLineOption`（含未导出 `extraValidation`/`minValue`/`listPreserveFalsyValues` 等） | `struct CommandLineOption { ... }`（声明表静态数据） | 声明表是 `&'static [CommandLineOption]`（`LazyLock`/`OnceLock` 构建）。关联映射（EnumMap/Elements/DeprecatedKeys）按 name 查全局表 |
| `CommandLineOptionKind`（string 常量 `"string"`/`"enum"`…） | `enum CommandLineOptionKind { String, Number, Boolean, Object, List, ListOrElement, Enum }` | Go 比较时常写 `opt.Kind == "boolean"` 字面量；Rust 用枚举判别 |
| `*diagnostics.Message` 指针字段 | `&'static diagnostics::Message`（或 `Option<&'static ...>`） | 声明表里 Description/Category 是诊断消息常量引用 |
| 各 `var XxxMap = collections.NewOrderedMapFromList(...)`（lib/target/...） | `static XXX_MAP: LazyLock<IndexMap<&'static str, EnumVal>>` | 字符串键→枚举值；保序（影响 "did you mean" 列表、`--init` 首项） |
| `reflect`（`mergeCompilerOptions`/`serializeCompilerOptions`/`ForEachCompilerOptionValue`/`parsinghelpers_test`） | **不用反射**：用 `core::CompilerOptions` 字段名↔声明的显式表 + 手写 visitor，或宏生成 | **关键偏离**：Go 大量 `reflect` 遍历 `CompilerOptions` 字段做合并/序列化/字段校验。Rust 改为显式字段表或派生宏。见偏离 |
| `floatOrInt32ToFlag[T ~int32](value any) T` | 泛型 `fn float_or_int_to_flag<T: From<i32>>(v: &OptionValue) -> T` | JSON number(f64)/已是枚举 → 枚举（`#[repr(i32)]`） |
| `sync.Once`（`ParsedCommandLine` 的 ~10 个惰性缓存） | `OnceLock<T>` / `OnceCell<T>`（各缓存字段） | wildcardDirectories/includeGlobs/locale/commonSourceDirectory/... |
| `iter.Seq`/`iter.Seq2`（`GetOutputFileNames` 等） | `impl Iterator<Item=...>`（或返回 `Vec`） | Go 1.23 range-over-func |
| `extendedConfigPath any`（string \| []string） | `enum ExtendedConfigPath { None, One(String), Many(Vec<String>) }` | `parsedTsconfig.extendedConfigPath` |
| `panic("List of ... not yet supported")` | `unreachable!()` / `panic!()` | 保留同样的"不支持即 panic"语义（这些路径当前不可达） |

### 选项声明表的表达

Go 用切片字面量（`commonOptionsWithBuild`、`optionsForCompiler` 等）+ `slices.Concat` 组装 `OptionsDeclarations`/`BuildOpts`。Rust 用 `static OPTIONS_DECLARATIONS: LazyLock<Vec<CommandLineOption>>`（或 `&[CommandLineOption]` 常量数组）。关联的 name→声明映射（`CompilerNameMap`/`BuildNameMap`/`WatchNameMap`/`CommandLineCompilerOptionsMap`）用 `LazyLock<NameMap>` 从声明表构建。

> **不变量**：`parsinghelpers_test.go:TestParseCompilerOptionNoMissingFields` 与 `decls_test.go:TestCompilerOptionsDeclaration` 要求 `core::CompilerOptions` 的每个导出字段都在 `parse_compiler_options` 的 switch 里有分支、且都有对应声明（除内部/skipped 列表）。Rust 侧若用枚举/宏，需保证这两个一致性 gate 仍能跑（见 tests.md）。

### 反射替代方案（务必读）

Go 在 4 处依赖 `reflect`，Rust 必须显式化：

1. `mergeCompilerOptions`（`parsinghelpers.go`）：遍历字段，按 json tag 判断显式 null → 清零，否则非零字段覆盖。
2. `serializeCompilerOptions`（`showconfig.go`）：遍历字段，跳过未设/命令行专属，按声明类型序列化（枚举→字符串名、路径→相对路径、Tristate→bool）。
3. `ForEachCompilerOptionValue`/`optionsHaveChanges`（`declscompiler.go`）：遍历字段做 affects-* 比较（strict/allowJs 特殊）。
4. 两个一致性测试遍历字段名。

推荐：定义一个**字段访问表**（`(json_name, getter, setter, decl_ref)` 列表）或用 `derive` 宏在 `core::CompilerOptions` 上生成 `for_each_field` / `field_by_json_name`。本包 impl.md 标 `// PERF(port)` / `// TODO(port)`，结构 1:1，仅遍历机制偏离。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/tsoptions/tsconfigparsing.go` | `internal/tsoptions/tsconfigparsing.rs` | tsconfig 解析主路径（最大文件，extends/configDir/filespecs/通配展开/references） |
| `internal/tsoptions/commandlineparser.go` | `internal/tsoptions/commandlineparser.rs` | 命令行/build 命令行解析、响应文件、`parseOptionValue` |
| `internal/tsoptions/parsinghelpers.go` | `internal/tsoptions/parsinghelpers.rs` | 值解析器（Tristate/StringArray/Number/...）、`ParseCompilerOptions` 巨型 switch、合并、绝对路径转换 |
| `internal/tsoptions/parsedcommandline.go` | `internal/tsoptions/parsedcommandline.rs` | `ParsedCommandLine` + 惰性缓存（wildcard/globs/locale/commonSrcDir/...） |
| `internal/tsoptions/parsedbuildcommandline.go` | `internal/tsoptions/parsedbuildcommandline.rs` | `ParsedBuildCommandLine` |
| `internal/tsoptions/commandlineoption.go` | `internal/tsoptions/commandlineoption.rs` | `CommandLineOption` 类型 + Elements/EnumMap/DeprecatedKeys 关联映射 |
| `internal/tsoptions/namemap.go` | `internal/tsoptions/namemap.rs` | `NameMap`（name/short→声明），`GetNameMapFromList` |
| `internal/tsoptions/enummaps.go` | `internal/tsoptions/enummaps.rs` | LibMap/target/module/jsx/newLine/watch* 枚举映射 + `GetLibFileName`/`GetDefaultLibFileName`/`TargetToLibMap` |
| `internal/tsoptions/diagnostics.go` | `internal/tsoptions/diagnostics.rs` | DidYouMean/AlternateMode/Worker 诊断声明（compiler/watch/build） |
| `internal/tsoptions/errors.go` | `internal/tsoptions/errors.rs` | 未知选项错误、无效枚举诊断、节点诊断工厂、extraKey 诊断 |
| `internal/tsoptions/wildcarddirectories.go` | `internal/tsoptions/wildcarddirectories.rs` | `getWildcardDirectories`（监视目录推导，递归/非递归） |
| `internal/tsoptions/showconfig.go` | `internal/tsoptions/showconfig.rs` | `ConvertToTSConfig`（`--showConfig`）+ implied options + 序列化 |
| `internal/tsoptions/declscompiler.go` | `internal/tsoptions/declscompiler.rs` | compiler 选项声明表 + affects-* 比较（reflect 替代） |
| `internal/tsoptions/declsbuild.go` | `internal/tsoptions/declsbuild.rs` | build 选项声明（TscBuildOption / OptionsForBuild / BuildOpts） |
| `internal/tsoptions/declswatch.go` | `internal/tsoptions/declswatch.rs` | watch 选项声明（OptionsForWatch） |
| `internal/tsoptions/declstypeacquisition.go` | `internal/tsoptions/declstypeacquisition.rs` | typeAcquisition 声明 |
| `internal/tsoptions/tsoptionstest/vfsparseconfighost.go` | `internal/tsoptions/tsoptionstest/vfsparseconfighost.rs`（子 module，仅测试用） | `VfsParseConfigHost` 测试 host |
| `internal/tsoptions/tsoptionstest/parsedcommandline.go` | `internal/tsoptions/tsoptionstest/parsedcommandline.rs`（子 module，仅测试用） | `GetParsedCommandLine` 测试帮手 |

> crate 入口 `lib.rs`：因为没有一个非测试文件 basename == `tsoptions`，需新建 `internal/tsoptions/lib.rs` 作为 crate 根，`mod` 声明上面各文件并 re-export 公开项。`tsoptionstest` 作为 `#[cfg(test)]`（或 `pub mod tsoptionstest`，因被外部 `_test` 用）子模块。

## 依赖白名单（本包新增的 crate）

- `indexmap`（已在 §10）——保序 map（解析中间表示、enum 映射、raw）。
- `rustc_hash`（已在 §10）——无序 set/map（去重等）。
- 派生宏（可选，自实现）替代 `reflect`：可考虑 `proc-macro` 在 `core` 包给 `CompilerOptions` 派生字段遍历；若不引宏，则手写字段表。记到 `references/crate-map.md`（标"反射替代"）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按包内依赖序：声明/映射数据 → 名字映射 → 值解析 → 命令行 → tsconfig → 结果对象 → showconfig。

### `commandlineoption.rs`（Go: `commandlineoption.go`）

- [ ] `pub enum CommandLineOptionKind { String, Number, Boolean, Object, List, ListOrElement, Enum }`　`// Go: commandlineoption.go`
- [ ] `pub struct CommandLineOption { name, short_name, kind, is_file_path, is_tsconfig_only, is_command_line_only, description, default_value_description, show_in_simplified_help_view, category, extra_validation, min_value, allow_config_dir_template_substitution, affects_*(8), allow_js_flag, strict_flag, transpile_option_value, list_preserve_falsy_values, element_options }`　`// Go: commandlineoption.go:CommandLineOption`
- [ ] `enum ExtraValidation { None, Spec, Locale }`　`// Go: commandlineoption.go:extraValidation`
- [ ] `fn deprecated_keys(&self) -> Option<&Set<String>>`（enum 才有）　`// Go: commandlineoption.go:DeprecatedKeys`
- [ ] `fn enum_map(&self) -> Option<&IndexMap<&str, EnumVal>>`　`// Go: commandlineoption.go:EnumMap`
- [ ] `fn elements(&self) -> Option<&CommandLineOption>`（list/listOrElement）　`// Go: commandlineoption.go:Elements`
- [ ] `fn disallow_null_or_undefined(&self) -> bool`（name=="extends"）　`// Go: commandlineoption.go:DisallowNullOrUndefined`
- [ ] `static COMMAND_LINE_OPTION_ELEMENTS`（lib/rootDirs/typeRoots/types/.../libFiles）　`// Go: commandlineoption.go:commandLineOptionElements`
- [ ] `static COMMAND_LINE_OPTION_ENUM_MAP`（lib/moduleResolution/module/target/.../fallbackPolling）　`// Go: commandlineoption.go:commandLineOptionEnumMap`
- [ ] `static COMMAND_LINE_OPTION_DEPRECATED`（module/moduleResolution/target）　`// Go: commandlineoption.go:commandLineOptionDeprecated`

### `enummaps.rs`（Go: `enummaps.go`）

- [ ] `static LIB_MAP`（~120 项 lib 名→`lib.*.d.ts`，保序）　`// Go: enummaps.go:LibMap`
- [ ] `static LIBS` / `static LIB_FILES_SET`　`// Go: enummaps.go:Libs/LibFilesSet`
- [ ] `fn get_lib_file_name(lib_name) -> Option<String>`　`// Go: enummaps.go:GetLibFileName`
- [ ] `static MODULE_RESOLUTION_OPTION_MAP` / `TARGET_OPTION_MAP` / `MODULE_OPTION_MAP` / `MODULE_DETECTION_OPTION_MAP` / `JSX_OPTION_MAP` / `NEW_LINE_OPTION_MAP`　`// Go: enummaps.go`（各 var）
- [ ] `static TARGET_TO_LIB_MAP` + `fn target_to_lib_map()` + `fn get_default_lib_file_name(options) -> String`　`// Go: enummaps.go:GetDefaultLibFileName`
- [ ] `static WATCH_FILE_ENUM_MAP` / `WATCH_DIRECTORY_ENUM_MAP` / `FALLBACK_ENUM_MAP`　`// Go: enummaps.go`

### `declscompiler.rs` / `declsbuild.rs` / `declswatch.rs` / `declstypeacquisition.rs`（Go 同名）

- [ ] `static COMMON_OPTIONS_WITH_BUILD` + `static OPTIONS_FOR_COMPILER` + `static OPTIONS_DECLARATIONS = concat`　`// Go: declscompiler.go`
- [ ] `fn options_have_changes(old, new, decl_filter) -> bool`（**reflect 替代**：字段表遍历）　`// Go: declscompiler.go:optionsHaveChanges`
- [ ] `fn for_each_compiler_option_value(options, decl_filter, fn) -> bool`（**reflect 替代**）　`// Go: declscompiler.go:ForEachCompilerOptionValue`
- [ ] `pub fn compiler_options_affect_semantic_diagnostics/declaration_path/emit(old,new) -> bool`　`// Go: declscompiler.go`
- [ ] `static TSC_BUILD_OPTION` + `static OPTIONS_FOR_BUILD` + `static BUILD_OPTS = concat`　`// Go: declsbuild.go`
- [ ] `static OPTIONS_FOR_WATCH`　`// Go: declswatch.go`
- [ ] `static TYPE_ACQUISITION_DECLARATION` + `static TYPE_ACQUISITION_DECLS`　`// Go: declstypeacquisition.go`

### `namemap.rs`（Go: `namemap.go`）

- [ ] `static COMPILER_NAME_MAP` / `BUILD_NAME_MAP` / `WATCH_NAME_MAP`（`LazyLock`，从声明表构建）　`// Go: namemap.go`
- [ ] `pub fn get_name_map_from_list(decls: &[CommandLineOption]) -> NameMap`　`// Go: namemap.go:GetNameMapFromList`
- [ ] `pub struct NameMap { options_names: IndexMap<String,&CommandLineOption>, short_option_names: FxHashMap<String,String> }`　`// Go: namemap.go:NameMap`
- [ ] `fn get(name)`/`get_from_short(short)`/`get_option_declaration_from_name(name, allow_short)`（全部小写化）　`// Go: namemap.go`

### `diagnostics.rs`（Go: `diagnostics.go`）

- [ ] `struct DidYouMeanOptionsDiagnostics`/`AlternateModeDiagnostics`/`ParseCommandLineWorkerDiagnostics`（含 `OnceLock` 替 `sync.Once`）　`// Go: diagnostics.go`
- [ ] `static COMPILER_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS` + `fn get_parse_command_line_worker_diagnostics(decls)`　`// Go: diagnostics.go`
- [ ] `static WATCH_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS` / `BUILD_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS`　`// Go: diagnostics.go`

### `errors.rs`（Go: `errors.go`）

- [ ] `fn create_diagnostic_for_invalid_enum_type(opt, source_file, node) -> Diagnostic`　`// Go: errors.go:createDiagnosticForInvalidEnumType`
- [ ] `fn format_enum_type_keys(opt, keys) -> String`（过滤 deprecated，`'a', 'b'` 格式）　`// Go: errors.go:formatEnumTypeKeys`
- [ ] `fn get_compiler_option_value_type_string(opt) -> String`　`// Go: errors.go:getCompilerOptionValueTypeString`
- [ ] `fn create_unknown_option_error(...) -> Diagnostic`（含 alternateMode/build-first 特判）　`// Go: errors.go:createUnknownOptionError`
- [ ] `pub fn create_diagnostic_for_node_in_source_file(...)` / `..._or_compiler_diagnostic(...)`　`// Go: errors.go`
- [ ] `fn extra_key_diagnostics(s)` / `extra_key_did_you_mean_diagnostics(s)`　`// Go: errors.go`

### `parsinghelpers.rs`（Go: `parsinghelpers.go`）

- [ ] `pub fn parse_tristate(value: &OptionValue) -> Tristate`　`// Go: parsinghelpers.go:ParseTristate`
- [ ] `pub fn parse_string_array(value) -> Option<Vec<String>>`　`// Go: parsinghelpers.go:ParseStringArray`
- [ ] `fn parse_string_map(value) -> Option<IndexMap<String,Vec<String>>>`　`// Go: parsinghelpers.go:parseStringMap`
- [ ] `pub fn parse_string(value) -> String`　`// Go: parsinghelpers.go:ParseString`
- [ ] `fn parse_number(value) -> Option<i32>`　`// Go: parsinghelpers.go:parseNumber`
- [ ] `fn parse_project_reference(json) -> Vec<ProjectReference>`　`// Go: parsinghelpers.go:parseProjectReference`
- [ ] `fn parse_json_to_string_key(json) -> IndexMap<String,OptionValue>`（include/exclude/files/references/extends/compilerOptions/excludes/typeAcquisition）　`// Go: parsinghelpers.go:parseJsonToStringKey`
- [ ] `trait OptionParser`（`parse_option`/`unknown_option_diagnostic`/`unknown_did_you_mean_diagnostic`）+ impls：`CompilerOptionsParser`/`WatchOptionsParser`/`TypeAcquisitionParser`/`BuildOptionsParser`　`// Go: parsinghelpers.go`
- [ ] `pub fn parse_compiler_options(key, value, all_options) -> bool`（**巨型 switch ~150 分支**；返回 foundKey）　`// Go: parsinghelpers.go:parseCompilerOptions`
- [ ] `pub fn parse_compiler_options_pub(key, value, all_options) -> Vec<Diagnostic>`（公开包装）　`// Go: parsinghelpers.go:ParseCompilerOptions`
- [ ] `fn float_or_int_to_flag<T>(value) -> T`　`// Go: parsinghelpers.go:floatOrInt32ToFlag`
- [ ] `pub fn parse_watch_options(key,value,all)` / `parse_type_acquisition(...)` / `parse_build_options(...)`　`// Go: parsinghelpers.go`
- [ ] `fn merge_compiler_options(target, source, raw_source) -> &CompilerOptions`（**reflect 替代**：含显式 null 清零）　`// Go: parsinghelpers.go:mergeCompilerOptions`
- [ ] `fn convert_to_options_with_absolute_paths(base, option_map, cwd)`　`// Go: parsinghelpers.go:convertToOptionsWithAbsolutePaths`
- [ ] `pub fn convert_option_to_absolute_path(o, v, option_map, cwd) -> Option<OptionValue>`　`// Go: parsinghelpers.go:ConvertOptionToAbsolutePath`

### `commandlineparser.rs`（Go: `commandlineparser.go`）

- [ ] `struct CommandLineParser { worker_diagnostics, options_map, fs, current_directory, options: IndexMap<String,OptionValue>, file_names, errors }`　`// Go: commandlineparser.go:commandLineParser`
- [ ] accessors `alternate_mode`/`options_declarations`/`unknown_option_diagnostic`/`unknown_did_you_mean_diagnostic`　`// Go: commandlineparser.go`
- [ ] `pub fn parse_command_line(command_line, host) -> ParsedCommandLine`（worker → 绝对路径 → 转 compiler/watch options → NewParsedCommandLine）　`// Go: commandlineparser.go:ParseCommandLine`
- [ ] `pub fn parse_build_command_line(command_line, host) -> ParsedBuildCommandLine`（含 clean/force/watch/dry 互斥诊断、空 projects 默认 `.`）　`// Go: commandlineparser.go:ParseBuildCommandLine`
- [ ] `fn parse_command_line_worker(diags, command_line, fs, cwd) -> CommandLineParser`　`// Go: commandlineparser.go:parseCommandLineWorker`
- [ ] `fn parse_strings(&mut self, args)`（`@`响应文件 / `-`选项（compiler 否则 watch 否则未知）/ 文件名）　`// Go: commandlineparser.go:parseStrings`
- [ ] `fn get_input_option_name(input) -> &str`（去最多两个前导 `-`）　`// Go: commandlineparser.go:getInputOptionName`
- [ ] `fn parse_response_file(&mut self, file_name)`（读文件、按空白/引号切词、未闭合引号诊断）　`// Go: commandlineparser.go:parseResponseFile`
- [ ] `fn try_read_file(file_name, read_file, errors) -> (String, Vec<Diagnostic>)`　`// Go: commandlineparser.go:tryReadFile`
- [ ] `fn parse_option_value(&mut self, args, i, opt, diag) -> usize`（**核心**：tsconfig-only/null/boolean/number/string/list/enum 各分支 + 缺参处理）　`// Go: commandlineparser.go:parseOptionValue`
- [ ] `pub fn parse_list_type_option(opt, value) -> (Vec<OptionValue>, Vec<Diagnostic>)`（`-`前缀空、listOrElement 单值、逗号切、string/enum 过滤）　`// Go: commandlineparser.go:ParseListTypeOption`
- [ ] `fn convert_json_option_of_enum_type(opt, value, value_expr, source_file) -> (OptionValue, Vec<Diagnostic>)`　`// Go: commandlineparser.go:convertJsonOptionOfEnumType`

### `wildcarddirectories.rs`（Go: `wildcarddirectories.go`）

- [ ] `pub fn get_wildcard_directories(include, exclude, compare_paths_options) -> Option<IndexMap<String,bool>>`（递归判定 + 子路径剔除）　`// Go: wildcarddirectories.go:getWildcardDirectories`
- [ ] `fn to_canonical_key(path, use_case_sensitive) -> String`　`// Go: wildcarddirectories.go:toCanonicalKey`
- [ ] `struct WildcardDirectoryMatch { key, path, recursive }` + `fn get_wildcard_directory_from_spec(spec, ucs) -> Option<...>`　`// Go: wildcarddirectories.go:getWildcardDirectoryFromSpec`

### `tsconfigparsing.rs`（Go: `tsconfigparsing.go`，最大）

声明 / 类型：

- [ ] `static COMPILER_OPTIONS_DECLARATION` / `COMPILE_ON_SAVE_COMMAND_LINE_OPTION` / `EXTENDS_OPTION_DECLARATION` / `TSCONFIG_ROOT_OPTIONS_MAP`　`// Go: tsconfigparsing.go`（var 块）
- [ ] `struct ConfigFileSpecs { ... validated*Specs ... is_default_include_spec }` + `matches_exclude`/`get_matched_include_spec`/`get_matched_file_spec`　`// Go: tsconfigparsing.go:configFileSpecs`
- [ ] `pub struct FileExtensionInfo { extension, is_mixed_content, script_kind }`　`// Go: tsconfigparsing.go:FileExtensionInfo`
- [ ] `pub trait ExtendedConfigCache { fn get_extended_config(...) -> ExtendedConfigCacheEntry }` + `pub struct ExtendedConfigCacheEntry`　`// Go: tsconfigparsing.go`
- [ ] `struct ParsedTsconfig { raw, options, type_acquisition, extended_config_path }`　`// Go: tsconfigparsing.go:parsedTsconfig`
- [ ] `pub struct TsConfigSourceFile { extended_source_files, config_file_specs, source_file }`　`// Go: tsconfigparsing.go:TsConfigSourceFile`
- [ ] `pub fn new_tsconfig_source_file_from_file_path(name, path, text) -> TsConfigSourceFile`　`// Go: tsconfigparsing.go:NewTsconfigSourceFileFromFilePath`
- [ ] `pub type CommandLineOptionNameMap` + `commandLineOptionsToMap` + `static COMMAND_LINE_COMPILER_OPTIONS_MAP`　`// Go: tsconfigparsing.go`

JSON → 选项转换：

- [ ] `fn parse_own_config_of_json_source_file(source_file, host, base_path, config_file_name) -> (ParsedTsconfig, Vec<Diagnostic>)`（含 `onPropertySet` 回调）　`// Go: tsconfigparsing.go:parseOwnConfigOfJsonSourceFile`
- [ ] `fn parse_own_config_of_json(json, host, base_path, config_file_name) -> (ParsedTsconfig, Vec<Diagnostic>)`　`// Go: tsconfigparsing.go:parseOwnConfigOfJson`
- [ ] `fn convert_config_file_to_object(source_file, notifier) -> (OptionValue, Vec<Diagnostic>)`（root 非 object 的错误恢复）　`// Go: tsconfigparsing.go:convertConfigFileToObject`
- [ ] `fn is_compiler_options_value(opt, value) -> bool`　`// Go: tsconfigparsing.go:isCompilerOptionsValue`
- [ ] `fn validate_json_option_value(opt, val, value_expr, source_file) -> (OptionValue, Vec<Diagnostic>)`（spec/locale extra validation）　`// Go: tsconfigparsing.go:validateJsonOptionValue`
- [ ] `fn convert_json_option_of_list_type(...)`（含 `listPreserveFalsyValues` 过滤）　`// Go: tsconfigparsing.go:convertJsonOptionOfListType`
- [ ] `fn starts_with_config_dir_template(value) -> bool` + `const CONFIG_DIR_TEMPLATE="${configDir}"`　`// Go: tsconfigparsing.go:startsWithConfigDirTemplate`
- [ ] `fn normalize_non_list_option_value(opt, base_path, value) -> OptionValue`　`// Go: tsconfigparsing.go:normalizeNonListOptionValue`
- [ ] `fn convert_json_option(opt, value, base_path, prop_assignment, value_expr, source_file) -> (OptionValue, Vec<Diagnostic>)`　`// Go: tsconfigparsing.go:convertJsonOption`
- [ ] `fn get_extends_config_path_or_array(...) -> (Vec<String>, Vec<Diagnostic>)` + `get_extends_config_path(...)`（rooted/relative vs module 解析）　`// Go: tsconfigparsing.go`
- [ ] `fn convert_map_to_options<O: OptionParser>(...)` / `convert_options_from_json<O>(...)`　`// Go: tsconfigparsing.go`
- [ ] `fn convert_array_literal_expression_to_json(...)` / `convert_object_literal_expression_to_json(...)` / `convert_to_json(...)` / `convert_property_value_to_json(...)`（JSON-AST → 值，含 number 取负、object/array 递归）　`// Go: tsconfigparsing.go`
- [ ] `fn is_double_quoted_string(node) -> bool` / `directory_of_combined_path(...)`　`// Go: tsconfigparsing.go`
- [ ] `pub fn parse_config_file_text_to_json(file_name, path, json_text) -> (OptionValue, Vec<Diagnostic>)`　`// Go: tsconfigparsing.go:ParseConfigFileTextToJson`
- [ ] `pub trait ParseConfigHost { fn fs(&self) -> &dyn Fs; fn get_current_directory(&self) -> &str; }`　`// Go: tsconfigparsing.go:ParseConfigHost`

extends / 主路径：

- [ ] `fn read_json_config_file(file_name, path, read_file) -> (TsConfigSourceFile, Vec<Diagnostic>)`　`// Go: tsconfigparsing.go:readJsonConfigFile`
- [ ] `fn get_extended_config(...) -> (Option<ParsedTsconfig>, Vec<Diagnostic>)`（含**循环检测时绕过 cache 防死锁**）　`// Go: tsconfigparsing.go:getExtendedConfig`
- [ ] `pub fn parse_extended_config(file_name, path, resolution_stack, host, cache) -> ExtendedConfigCacheEntry`　`// Go: tsconfigparsing.go:ParseExtendedConfig`
- [ ] `fn parse_config(json, source_file, host, base_path, config_file_name, resolution_stack, cache) -> (ParsedTsconfig, Vec<Diagnostic>)`（**extends 合并核心** + 循环诊断 + include/exclude/files 相对路径重写 + `applyExtendedConfig`）　`// Go: tsconfigparsing.go:parseConfig`
- [ ] `pub fn parse_json_source_file_config_file_content(...) -> ParsedCommandLine`　`// Go: tsconfigparsing.go:ParseJsonSourceFileConfigFileContent`
- [ ] `pub fn parse_json_config_file_content(json, host, base_path, ...) -> ParsedCommandLine`　`// Go: tsconfigparsing.go:ParseJsonConfigFileContent`
- [ ] `fn parse_json_config_file_content_worker(...) -> ParsedCommandLine`（**主编排**：basePath/合并existing/configDir 替换/`getPropFromRaw` files/include/exclude/references/空文件诊断/默认 include/规格校验/文件展开）　`// Go: tsconfigparsing.go:parseJsonConfigFileContentWorker`
- [ ] `fn can_json_report_no_input_files(raw)` / `should_report_no_input_files(...)`　`// Go: tsconfigparsing.go`
- [ ] `fn validate_specs(specs, disallow_trailing_recursion, source_file, key) -> (Vec<String>, Vec<Diagnostic>)` + `spec_to_diagnostic` + `invalid_trailing_recursion` + `invalid_dot_dot_after_recursive_wildcard`　`// Go: tsconfigparsing.go`
- [ ] tsconfig prop 取值帮手：`get_ts_config_prop_array_element_value` / `for_each_ts_config_prop_array` / `create_diagnostic_at_reference_syntax` / `get_callback_for_finding_property_assignment_by_value` / `get_options_syntax_by_array_element_value` / `for_each_property_assignment` / `get_ts_config_object_literal_expression`　`// Go: tsconfigparsing.go`
- [ ] `${configDir}` 替换：`get_substituted_path_with_config_dir_template` / `get_substituted_string_array_with_config_dir_template` / `handle_option_config_dir_template_substitution`　`// Go: tsconfigparsing.go`
- [ ] 扩展名优先级：`has_file_with_higher_priority_extension` / `remove_wildcard_files_with_lower_priority_extension`　`// Go: tsconfigparsing.go`
- [ ] `fn get_file_names_from_config_specs(specs, base_path, options, host, extra_extensions) -> (Vec<String>, usize)`（**通配展开核心**：literal/wildcard/json 三 map + `vfsmatch.ReadDirectory` + 优先级去重）　`// Go: tsconfigparsing.go:getFileNamesFromConfigSpecs`
- [ ] `pub fn get_supported_extensions(options, extra) -> Vec<Vec<String>>` / `get_supported_extensions_with_json_if_resolve_json_module(...)`　`// Go: tsconfigparsing.go`
- [ ] `pub fn get_parsed_command_line_of_config_file(...)` / `get_parsed_command_line_of_config_file_path(...)`　`// Go: tsconfigparsing.go`

### `parsedcommandline.rs`（Go: `parsedcommandline.go`）

- [ ] `pub struct ParsedCommandLine { parsed_config, config_file, errors, raw, compile_on_save, compare_paths_options, + ~10 个 OnceLock 缓存, ... }`　`// Go: parsedcommandline.go:ParsedCommandLine`
- [ ] `pub fn new_parsed_command_line(compiler_options, root_file_names, compare_paths_options)`　`// Go: parsedcommandline.go:NewParsedCommandLine`
- [ ] `pub struct SourceOutputAndProjectReference { source, output_dts, resolved }`　`// Go: parsedcommandline.go`
- [ ] accessors/惰性方法：`config_name`/`source_to_project_reference`/`output_dts_to_project_reference`/`parse_input_output_names`/`common_source_directory`/`check_source_files_belong_to_path`/`get_current_directory`/`use_case_sensitive_file_names`/`get_output_declaration_and_source_file_names`/`get_output_file_names`/`get_build_info_file_name`/`wildcard_directories`/`wildcard_directory_globs`/`literal_file_names`/`set_parsed_options`/`set_compiler_options`/`compiler_options`/`set_type_acquisition`/`type_acquisition`/`file_names`/`file_names_by_path`/`project_references`/`resolved_project_reference_paths`/`extended_source_files`/`get_config_file_parsing_diagnostics`/`possibly_matches_file_name`/`possibly_matches_directory_name`/`get_matched_file_spec`/`get_matched_include_spec`/`reload_file_names_of_parsed_command_line`/`locale`　`// Go: parsedcommandline.go`（逐方法）
- [ ] `const FILE_GLOB_PATTERN` / `RECURSIVE_FILE_GLOB_PATTERN`　`// Go: parsedcommandline.go`

### `parsedbuildcommandline.rs`（Go: `parsedbuildcommandline.go`）

- [ ] `pub struct ParsedBuildCommandLine { build_options, compiler_options, watch_options, projects, errors, raw, compare_paths_options, OnceLock 缓存 }` + `resolved_project_paths`/`locale`　`// Go: parsedbuildcommandline.go`

### `showconfig.rs`（Go: `showconfig.go`）

- [ ] `fn compute_fn<T>(...)` + `struct ImpliedOption { name, dependencies, compute }` + `static IMPLIED_OPTIONS`　`// Go: showconfig.go`
- [ ] `pub struct TSConfig { compiler_options, references, files, include, exclude, compile_on_save }`　`// Go: showconfig.go:TSConfig`
- [ ] `pub fn convert_to_tsconfig(config_parse_result, config_file_name) -> TSConfig`　`// Go: showconfig.go:ConvertToTSConfig`
- [ ] `fn filter_same_as_default_include(specs)` / `get_name_of_compiler_option_value(value, enum_map)`　`// Go: showconfig.go`
- [ ] `fn serialize_compiler_options(options, config_path, cpo) -> IndexMap<String,OptionValue>`（**reflect 替代**）　`// Go: showconfig.go:serializeCompilerOptions`
- [ ] `fn serialize_enum_value(value, enum_map) -> String` / `add_implied_options(...)` / `any_dependency_provided(...)` / `serialize_implied_option_value(...)`　`// Go: showconfig.go`

### `tsoptionstest/`（仅测试用子模块）

- [ ] `pub struct VfsParseConfigHost { vfs, current_directory }` + impl `ParseConfigHost` + `new_vfs_parse_config_host(files, cwd, ucs)` + `fix_root`　`// Go: tsoptionstest/vfsparseconfighost.go`
- [ ] `pub fn get_parsed_command_line(json_text, files, cwd, ucs) -> ParsedCommandLine`　`// Go: tsoptionstest/parsedcommandline.go`

### export 等价物（Go: `export_test.go` 暴露内部供 `_test` 包用）

- [ ] 测试可见的 `ParseCommandLineTestWorker` / `TestCommandLineParser`（Rust 用 `#[cfg(test)] pub(crate)` 或 `pub` 测试帮手）　`// Go: export_test.go`

### Cargo / crate 接线

- [ ] `internal/tsoptions/Cargo.toml`（`name = "tsgo_tsoptions"` + 全部 path deps）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `internal/tsoptions/lib.rs`（crate 根）：`mod` 各文件 + re-export 公开 API（`ParseCommandLine`/`ParseBuildCommandLine`/`ParseJsonSourceFileConfigFileContent`/`ParsedCommandLine`/`CommandLineOption`/`OptionsDeclarations`/...）

## TDD 推进顺序（tracer bullet → 增量）

1. **枚举/名字映射**：`enummaps` + `namemap`（`get_lib_file_name`/`NameMap::get`）——纯函数，无依赖，先红绿（tracer bullet）。
2. **值解析器**：`parse_tristate`/`parse_string`/`parse_string_array`/`parse_number`。
3. **声明一致性 gate**：`parse_compiler_options` 巨型 switch + `TestParseCompilerOptionNoMissingFields`/`TestCompilerOptionsDeclaration`（确保字段全覆盖；这两个是后续所有解析正确性的地基）。
4. **命令行**：`parse_command_line`（单选项、lib 列表、boolean 显式值、缺参、tsconfig-only null、build 互斥）——对应 `TestCommandLineParseResult`/`TestParseBuildCommandLine`/`TestParseCommandLineVerifyNull`。
5. **wildcard 目录**：`get_wildcard_directories`（含非 ASCII 路径）。
6. **tsconfig 解析**：`parse_config_file_text_to_json`（注释/空白/lib）→ `parse_json_config_file_content_worker`（files/include/exclude/extends/configDir/null 覆盖/通配展开/references）——对应 `TestParseConfigFileTextToJson`/`TestParseJsonConfigFileContent`/`TestParseJsonSourceFileConfigFileContent`/`TestParseTypeAcquisition`。
7. **结果对象惰性方法**：`possibly_matches_file_name`/`literal_file_names`/`reload_file_names_*`（对应 `TestParsedCommandLine`）。
8. **showConfig / extends cache**：`convert_to_tsconfig`、`TestExtendedConfigErrorsAppearOnCacheHit`。

## 与 Go 的已知偏离（divergence）

- **`any` → `OptionValue` 判别联合**：Go 解析中间值全是 `any`（`*OrderedMap[string,any]`、`[]any`、`string`/`bool`/`float64`）。Rust 用 `enum OptionValue` + `IndexMap`。`reflect.TypeOf(...).Kind()` 的类型判断改为 `match value`。结构 1:1，类型表达更显式。
- **`reflect` → 字段表/宏**：`mergeCompilerOptions`/`serializeCompilerOptions`/`ForEachCompilerOptionValue`/两个一致性测试都靠反射遍历 `CompilerOptions` 字段。Rust 改为显式字段访问表或 `derive` 宏。标 `// PERF(port)`，必须保证遍历到的字段集合、json 名、affects-* 行为与 Go 完全一致（gate 测试兜底）。
- **声明表 `var` 切片 → `LazyLock`**：选项声明、enum 映射、name 映射都是进程级静态数据，用 `LazyLock`/`OnceLock` 构建（Go 是包级 `var` 初始化）。
- **`sync.Once` 缓存 → `OnceLock`**：`ParsedCommandLine`/`ParsedBuildCommandLine` 的惰性字段逐一改 `OnceLock<T>`。注意 `&self` 方法仍能写入。
- **字符串 Kind 比较 → 枚举**：Go 频繁 `opt.Kind == "boolean"`；Rust 用 `CommandLineOptionKind::Boolean`。
- **`iter.Seq` → `Iterator`**：`get_output_file_names`/`get_output_declaration_and_source_file_names` 返回 `impl Iterator`（或先收 `Vec`）。
- **`extendedConfigPath any`（string|[]string）→ `enum`**：用 `ExtendedConfigPath` 三态。
- **`parseNumber` 截断**：Go `int(num)`（float→int 截断）。Rust 用 `as i32`，对齐截断语义，存疑处标 `// PERF(port)`。
- **`tryReadFile` 诊断信息缺失**：Go 注释标 `!!! Divergence: 错误信息不含有用消息`，统一用 `Cannot_read_file_0`。Rust 照搬该已知偏离。
- **`panic("listOrElement not supported here")` / `panic("List of ...")`**：保留为 `panic!`/`unreachable!`（当前不可达路径）。

## 转交 / 推迟（DEFER）

- **submodule 基线测试**：`TestParseSrcCompiler`/`TestParseConfigFileTextToJson`/部分 `TestParseJsonConfigFileContent` 依赖 TypeScript submodule + baseline 框架（`baseline.RunAgainstSubmodule`）。这些是端到端基线对拍，归 **P10 parity**；本 phase 只做 `noSubmoduleBaseline=true` 的内联用例 + 关键断言（见 tests.md）。
- **watchOptions 在 tsconfig 中的解析**：Go 源里 `watchOptions` 在 tsconfig 路径多处被注释掉（`// watchOptions`）。保持同样"暂未接入"状态，标 `// TODO(port)`，与 Go 行为一致。
- **`outputpaths`/`module`/`glob`/`vfsmatch` 依赖**：这些是 P1/P4 落地物；本包实现时若它们未就绪，相关函数（`getFileNamesFromConfigSpecs`/`getExtendsConfigPath` 的 module 解析/`GetBuildInfoFileName`）标 `// DEFER` blocked-by 对应包。
