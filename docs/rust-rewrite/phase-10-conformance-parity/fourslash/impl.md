# fourslash: 实现方案（impl.md）

**crate**：`tsgo_fourslash`　**目标**：**语言服务 parity 框架** —— 解析 TS fourslash DSL（`/*marker*/`、`[|range|]`、`{| obj |}`）→ 驱动 Rust LSP server → 用 `Verify*` 内联断言 + `VerifyBaseline*` 把语言服务响应渲染成 baseline，与 `testdata/baselines/reference/fourslash/**` 逐字节对拍。
**依赖（crate）**：`tsgo_lsp` `tsgo_ls`（含 `lsconv` / `lsutil`）`tsgo_project` `tsgo_lsp_lsproto` `tsgo_testutil`（`lsptestutil` / `harnessutil` / `tsbaseline` / `baseline`）`tsgo_testrunner`（复用 `parse_test_files_and_symlinks_with_options`）`tsgo_core` `tsgo_collections` `tsgo_tspath` `tsgo_vfs`（`vfstest` / `iovfs`）`tsgo_bundled` `tsgo_stringutil` `tsgo_json` `tsgo_diagnosticwriter` `tsgo_execute`（`tsctests`）`tsgo_jsonrpc`。
**Go 源**：`internal/fourslash/`（6 个核心 `.go`：`fourslash.go`(~5300 行) `test_parser.go` `baselineutil.go` `statebaseline.go` `semantictokens.go` `skip_if_failing.go` + `tests/util/util.go` 支撑 helper）+ **4250 个生成的 `*_test.go` / 4386 个 `func Test*`**（`tests/` 277 + `tests/gen/` 3768 + `tests/manual/` 205）

> ⚠️ **本文件不逐个枚举 4250 个测试。** 那 4250 个 Go 文件本身是从 TS 上游 fourslash 用例**生成**的；Rust 侧复用同一批 fixture，不翻译 Go 产物。下文重点是 **DSL 解析 + runner 框架 + 复用 fixture 的生成器思路**。

## 这个包是什么（业务说明）

fourslash 是 TypeScript 自家的**语言服务测试框架**。一个 fourslash 测试 = 一段带"魔法标记"的 TS 源码 + 一串对语言服务的断言。它覆盖了 `internal/ls`（go-to-definition、completion、find-references、hover、rename、signature help、inlay hints、code fix、folding、document symbol…）几乎全部 feature——这些 feature 在前序 phase 几乎没有直接单测，**全靠 fourslash 兜底**。

工作机制（Go 侧，`fourslash.go`）：

1. **解析 DSL**（`test_parser.go:ParseTestData` → `parseFileContent`）：把源码里的标记抠出来，得到纯净文件内容 + marker/range 列表：
   - `/*name*/` → **命名 marker**（`Marker{Position, Name}`），定位光标点。
   - `[|text|]` → **range marker**（`RangeMarker{Range}`），选区。range 内可嵌 marker：`[|/*m*/foo|]`。
   - `{| "key": val |}` → **对象 marker**（JSON 数据，可匿名）。
   - `// @Filename:` / `// @symlink:` 等指令复用 `testrunner.ParseTestFilesAndSymlinksWithOptions`（`AllowImplicitFirstFile: true`）。
2. **起一个内存 LSP server**（`NewFourslash`）：用 `lsptestutil.NewLSPClient` 建内存双向通道 client/server，挂内存 VFS（含 bundled lib），`initialize` + 打开所有文件。
3. **跑命令**：生成的测试调 `f.GoToMarker(t, "x")` 移动光标、`f.VerifyCompletions(...)` / `f.VerifyBaselineGoToDefinition(...)` 等发 LSP 请求并断言。
4. **两种断言风格**：
   - **内联断言**（`VerifyCompletions` / `VerifyQuickInfoAt` / `VerifySignatureHelp` …）：expected 直接写在 `_test.go` 里（completion item 列表、quickinfo 文本等），断言相等。
   - **baseline 断言**（`VerifyBaseline*`：FindAllReferences / GoToDefinition / Hover / SignatureHelp / Rename / CallHierarchy / DocumentHighlights / InlayHints / DocumentSymbol / CodeLens / SelectionRanges / Diagnostics …）：把响应渲染成文本累积进 `f.baselines[command]`，测试结束（`done()` → `verifyBaselines`）时调 `baseline.Run` 与 `reference/fourslash/<command>/<name>.baseline.jsonc` 对拍。
5. **state baseline 模式**（`statebaseline.go`，`// @statebaseline: true`）：把整段编辑/项目状态变化录成单一 `.baseline`。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING §3。本包特有：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `FourslashTest`（持 client / vfs / testData / baselines / scriptInfos / converters …） | `struct FourslashTest { client, vfs, test_data, baselines: IndexMap<BaselineCommand, String>, script_infos: IndexMap<String, ScriptInfo>, converters, ... }` | 核心驱动对象 |
| `Marker{Position, LSPosition, Name, Data}` / `RangeMarker{Range, LSRange, Marker}` | `struct Marker { position: i32, ls_position: Position, name: Option<String>, data: Option<JsonMap> }` / `RangeMarker` | DSL 解析产物；`name=None` 是匿名对象 marker |
| `MarkerOrRange` interface | `enum MarkerOrRange { Marker(Marker), Range(RangeMarker) }` 或 `trait` | `GoToMarkerOrRange` 用 |
| `baselineCommand string`（常量集） | `enum BaselineCommand` + `as_str()` | 决定 baseline 子目录 + 扩展名（`.baseline.jsonc` / `.baseline` / `.callHierarchy.txt` / `.baseline.md` / `.linkedEditing.txt`） |
| `map[baselineCommand]*strings.Builder` | `IndexMap<BaselineCommand, String>` | **每命令一个 baseline 缓冲**，顺序确定 |
| `lsptestutil.LSPClient`（内存 channel） | `tsgo_testutil::lsptestutil::LspClient` | 见 testutil/impl.md（fourslash 阻塞依赖） |
| `lsconv.Converters` / `LSPLineMap` | `tsgo_ls::lsconv::Converters` | position ↔ (line, char) 换算（UTF-8 编码） |
| `t.Name()` → 反推 baseline 文件名（`getBaseFileNameFromTest`） | `harness.test_name()` → 同算法（去 `Test` 前缀 + lowerFirst + 特例） | baseline 文件名由测试函数名反推 |
| `runtime.Caller(1)` 拿 testPath（判 submodule） | 调用方显式传 test 源文件路径 | submodule diff fixup 用 |
| 生成测试里的 `. "tests/util"`（DefaultCommitCharacters / CompletionGlobals…） | `tsgo_fourslash::tests::util`（公共 completion 常量集） | 巨大的 completion item 常量表（global vars/keywords/types），1:1 移植 |

### 偏离重点：4250 个生成测试 → 复用 fixture，不翻译

见下文"复用 fixture 的生成器思路"。核心约定：**Rust 不持有 4250 个手写测试**，而是复用同一批 TS fixture 重新生成 Rust 测试（或直接用现有 `.go` 生成产物作中间表示）。

## 文件清单 → Rust 模块

### 框架核心（6 文件，必须逐文件移植）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/fourslash/fourslash.go` | `internal/fourslash/lib.rs` | crate 根：`FourslashTest` + `NewFourslash` + 全部 `GoTo*` / `Verify*` / `VerifyBaseline*` / 编辑命令（Insert/Replace/Backspace…）+ `verifyBaselines` |
| `internal/fourslash/test_parser.go` | `internal/fourslash/test_parser.rs` | `ParseTestData` / `parseFileContent`（DSL 状态机）/ `Marker` / `RangeMarker` / `TestData` / `getObjectMarker` |
| `internal/fourslash/baselineutil.go` | `internal/fourslash/baselineutil.rs` | `baselineCommand` 常量 + `addResultToBaseline` / `getBaselineFileName` / `getBaselineExtension` / `getBaselineOptions`（含各命令的 submodule `DiffFixupOld`）|
| `internal/fourslash/statebaseline.go` | `internal/fourslash/statebaseline.rs` | `stateBaseline`（`@statebaseline` 模式：录项目/编辑状态） |
| `internal/fourslash/semantictokens.go` | `internal/fourslash/semantictokens.rs` | semantic token 类型/修饰符默认集 + 渲染 |
| `internal/fourslash/skip_if_failing.go` | `internal/fourslash/skip_if_failing.rs` | `SkipIfFailing`（读 `_scripts/failingTests.txt` 跳过已知失败；`TSGO_FOURSLASH_IGNORE_FAILING` 覆盖） |

### 支撑 / 生成产物（不逐文件翻译）

| Go 产物 | Rust 对应 | 说明 |
|---|---|---|
| `internal/fourslash/tests/util/util.go` | `internal/fourslash/tests/util/mod.rs` | **必须 1:1 移植**：`DefaultCommitCharacters` / `Ignored` / `CompletionGlobals` / `CompletionGlobalKeywords` / `CompletionGlobalTypes` / `CompletionFunctionMembers*` 等巨型常量 + `sortCompletionItems` / `CompletionGlobalsPlus`。生成测试大量引用 |
| `internal/fourslash/tests/{*,gen/*,manual/*}_test.go`（4250 文件） | **由生成器产出**，不手写 | 见"复用 fixture 的生成器思路" |
| `internal/fourslash/_scripts/convertFourslash.mts` | `internal/fourslash/_scripts/`（保留或改写） | 生成器本体；Rust 侧改其输出模板即可复用解析逻辑 |
| `_scripts/failingTests.txt`（457）/ `crashingTests.txt`（1）/ `manualTests.txt`（172）/ `unparsedTests.txt`（2603） | 原样保留（runner 读取） | 跳过名单 / 手工名单 / 未能解析名单 |

## 依赖白名单（本包新增的 crate）

| 用途 | crate | 备注 |
|---|---|---|
| JSON（对象 marker `{| ... |}` + baseline jsonc 渲染） | `serde_json` | 复用 P1 的 `tsgo_json` 优先 |
| UTF-8 字符迭代（DSL 状态机逐 rune） | std（`char_indices`） | Go 用 `utf8.DecodeRuneInString`；注意 marker 位置是 byte offset |
| 比较 / diff（cmp.Diff 风格断言） | `similar`（复用 testutil） | 内联断言失败时给可读 diff |

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 框架优先：先把"解析 DSL → 起 server → 出 1 个 baseline → 对拍"打穿（tracer bullet），再按命令族逐个补 `Verify*`。

### `test_parser.rs`（Go: `internal/fourslash/test_parser.go`）— DSL 解析（最先做）

- [ ] `struct Marker / RangeMarker / TestData / TestFileInfo`（+ `MarkerOrRange`）　`// Go: test_parser.go:*`
- [ ] `pub fn parse_test_data(h, contents, file_name) -> TestData` — 调 testrunner 的 `parse_test_files_and_symlinks_with_options`（AllowImplicitFirstFile）+ 收集 markers/ranges + 重名校验　`// Go: test_parser.go:ParseTestData`
- [ ] `fn parse_file_content(file_name, content, file_options) -> TestFileWithMarkers` — **核心状态机**：逐 rune 扫描，识别 `[|`/`|]`/`/*`/`*/`/`{|`/`|}`，维护 `difference`（已删元字符数）/ line / column；range 嵌 marker；`chompLeadingSpace`　`// Go: test_parser.go:parseFileContent`
- [ ] `fn get_object_marker(file_name, loc, text) -> Marker` — `{ <text> }` 当 JSON 解析；匿名/命名　`// Go: test_parser.go:getObjectMarker`
- [ ] marker LS position 换算（`ComputeLSPLineStarts` + `Converters.PositionToLineAndCharacter`）+ range 按 (pos, -end) 稳定排序　`// Go: test_parser.go:ParseTestData`(尾段)
- [ ] `is_state_baselining_enabled` / `is_config_file` / `has_unsupported_global_options_with_config`　`// Go: test_parser.go:*`

### `lib.rs`（Go: `internal/fourslash/fourslash.go`）— 驱动框架

- [ ] `struct ScriptInfo { file_name, content, line_map, version }` + `edit_content`（应用 TextChange）　`// Go: fourslash.go:scriptInfo`
- [ ] `pub fn new_fourslash(h, capabilities, content) -> (FourslashTest, impl FnOnce())` — 解析 testData → 建内存 VFS（文件 + symlink）→ 默认 compilerOptions → `SkipUnsupportedCompilerOptions` → 起 `lsptestutil` server → `initialize` → 打开文件 / 或 state baseline 初始化；返回的 `done` 闭包跑 `verify_baselines`　`// Go: fourslash.go:NewFourslash`
- [ ] `handle_server_request`（workspace/configuration → 返回 userPreferences；register/unregister capability → 接受）　`// Go: fourslash.go:handleServerRequest`
- [ ] `fn get_base_file_name_from_test(h) -> String`（去 Test 前缀 + lowerFirst + callHierarchyFunctionAmbiguity 特例）　`// Go: fourslash.go:getBaseFileNameFromTest`
- [ ] `initialize`（发 InitializeParams + InitializedParams + 等 InitComplete）　`// Go: fourslash.go:initialize`
- [ ] **导航命令**：`GoToMarker` / `GoToMarkerOrRange` / `GoToPosition` / `GoToEOF` / `GoToBOF` / `GoToEachMarker` / `GoToEachRange` / `GoToRangeStart` / `GoToSelect` / `GoToSelectRange` / `GoToFile` / `GoToFileNumber`　`// Go: fourslash.go:GoTo*`
- [ ] **marker 访问**：`Markers` / `MarkerNames` / `MarkerByName` / `Ranges` / `GetRangesByText` / `CloseFileOfMarker`　`// Go: fourslash.go:Markers`等
- [ ] **编辑命令**：`Insert` / `InsertLine` / `Backspace` / `DeleteAtCaret` / `Paste` / `ReplaceLine` / `Replace`（改 scriptInfo + 发 didChange）　`// Go: fourslash.go:Insert`等
- [ ] **内联断言命令族**（expected 写在测试里）：
  - [ ] `VerifyCompletions`（+ `GetCompletions` / `ResolveCompletionItem`；`CompletionsExpectedList` / `Includes`/`Excludes`/`Exact`）　`// Go: fourslash.go:VerifyCompletions`
  - [ ] `VerifyQuickInfoAt` / `VerifyQuickInfoIs` / `VerifyQuickInfoExists` / `VerifyNotQuickInfoExists`　`// Go: fourslash.go:VerifyQuickInfo*`
  - [ ] `VerifySignatureHelp` / `VerifyNoSignatureHelp*` / `VerifySignatureHelpPresent*` / `VerifySignatureHelpWithCases`　`// Go: fourslash.go:VerifySignatureHelp*`
  - [ ] `VerifyCodeFix` / `VerifyCodeFixAvailable[Exact]` / `VerifyCodeFixNotAvailable` / `VerifyCodeFixAll` / `VerifySourceFixAll` / `VerifyRangeAfterCodeFix`　`// Go: fourslash.go:VerifyCodeFix*`
  - [ ] `VerifyRename` / `VerifyRenameSucceeded` / `VerifyRenameFailed` / `RenameAtCaret` / `VerifyWillRenameFilesEdits` / `WillRenameFiles`　`// Go: fourslash.go:VerifyRename*`
  - [ ] `VerifyOrganizeImports` / `VerifyImportFixAtPosition` / `VerifyImportFixModuleSpecifiers` / `VerifyApplyCodeActionFromCompletion`　`// Go: fourslash.go:Verify*Import*`
  - [ ] `VerifyCurrentFileContent` / `VerifyCurrentLineContent` / `VerifyIndentation`　`// Go: fourslash.go:VerifyCurrent*`
  - [ ] `VerifyDiagnostics` / `VerifyNonSuggestionDiagnostics` / `VerifySuggestionDiagnostics` / `VerifyNoErrors` / `VerifyNumberOfErrorsInCurrentFile` / `VerifyErrorExistsAtRange`　`// Go: fourslash.go:VerifyDiagnostics`等
  - [ ] `VerifyFoldingRangeLines` / `VerifyOutliningSpans` / `VerifyJsxClosingTag` / `VerifyLinkedEditing` / `VerifyWorkspaceSymbol`　`// Go: fourslash.go:*`
  - [ ] `FormatDocument` / `FormatSelection`　`// Go: fourslash.go:Format*`
- [ ] **baseline 断言命令族**（写 baseline 缓冲，结束对拍）：
  - [ ] `VerifyBaselineFindAllReferences` / `VerifyBaselineVsFindAllReferences`　`// Go: fourslash.go:VerifyBaselineFindAllReferences`
  - [ ] `VerifyBaselineGoToDefinition` / `GoToTypeDefinition` / `GoToSourceDefinition` / `GoToImplementation`　`// Go: fourslash.go:VerifyBaselineGoTo*`
  - [ ] `VerifyBaselineHover` / `VerifyBaselineHoverWithVerbosity`　`// Go: fourslash.go:VerifyBaselineHover`
  - [ ] `VerifyBaselineSignatureHelp` / `VerifyBaselineSelectionRanges` / `VerifyBaselineCallHierarchy`　`// Go: fourslash.go:VerifyBaseline*`
  - [ ] `VerifyBaselineDocumentHighlights[WithOptions]` / `VerifyBaselineDocumentSymbol` / `VerifyBaselineClosingTags`　`// Go: fourslash.go:VerifyBaseline*`
  - [ ] `VerifyBaselineInlayHints` / `VerifyBaselineLinkedEditing` / `VerifyBaselineRename[AtRangesWithText]` / `VerifyBaselineCodeLens` / `VerifyBaselineWorkspaceSymbol`　`// Go: fourslash.go:VerifyBaseline*`
  - [ ] `VerifyBaselineNonSuggestionDiagnostics` / `BaselineAutoImportsCompletions`　`// Go: fourslash.go:*`
- [ ] `verify_baselines(h, test_path)` — 非 state 模式：每 command 调 `baseline::run(getBaselineFileName, content, getBaselineOptions)`；state 模式：单 `.baseline`　`// Go: fourslash.go:verifyBaselines`

### `baselineutil.rs`（Go: `internal/fourslash/baselineutil.go`）

- [ ] `enum BaselineCommand`（findAllReferences / goToDefinition / QuickInfo / SignatureHelp / Rename / Call Hierarchy / Inlay Hints / Document Symbols / …）　`// Go: baselineutil.go:常量`
- [ ] `add_result_to_baseline` / `write_to_baseline`（`// === <cmd> ===` 分隔 + `\n\n\n\n` 间隔）　`// Go: baselineutil.go:addResultToBaseline`
- [ ] `get_baseline_file_name` / `get_baseline_extension`（各命令扩展名映射）　`// Go: baselineutil.go:getBaselineExtension`
- [ ] `get_baseline_options`（subfolder=`fourslash/<cmd>`；submodule 模式各命令的 `DiffFixupOld`：路径前缀剥离 / SymbolKind 映射 / preference 改名 / inlay hint 字段重写…）　`// Go: baselineutil.go:getBaselineOptions`
- [ ] 渲染 helper：location/span 文本化、`dropTrailingEmptyLines` 等　`// Go: baselineutil.go:*`

### `statebaseline.rs` / `semantictokens.rs` / `skip_if_failing.rs`

- [ ] `struct StateBaseline`（`@statebaseline` 录状态）　`// Go: statebaseline.go:*`（可后置，仅少量测试用）
- [ ] semantic token 默认类型/修饰符集 + 渲染　`// Go: semantictokens.go:*`
- [ ] `skip_if_failing(h)` 读 `_scripts/failingTests.txt`　`// Go: skip_if_failing.go:SkipIfFailing`

### `tests/util/mod.rs`（Go: `internal/fourslash/tests/util/util.go`）

- [ ] `Ignored` / `DefaultCommitCharacters` / `CompletionGlobalThisItem` / `CompletionUndefinedVarItem`　`// Go: util.go:*`
- [ ] `CompletionGlobalVars` / `CompletionGlobalKeywords` / `CompletionGlobalTypeDecls` / `CompletionTypeKeywords` / `CompletionClassElementKeywords` / `CompletionConstructorParameterKeywords` / `CompletionFunctionMembers[WithPrototype]`（巨型常量表，1:1）　`// Go: util.go:*`
- [ ] `sortCompletionItems` / `CompletionGlobals[Plus]` / `CompletionGlobalTypes[Plus]` / `CompletionGlobalsInJSPlus` / `getInJSKeywords` / `ToAny`　`// Go: util.go:*`

### Cargo / crate 接线

- [ ] `internal/fourslash/Cargo.toml`（`name = "tsgo_fourslash"` + path deps）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 子模块声明 + re-export；`tests/util` 作 `pub mod`

## 复用 fixture 的生成器思路（核心：绝不翻译 4250 个 Go 文件）

Go 的 4250 个 `*_test.go` **不是手写**的，是 `_scripts/convertFourslash.mts` 从 `_submodules/TypeScript/tests/cases/fourslash/*.ts`（TS 上游 fourslash 用例，~2600+ 个）**生成**的：

```
TS 上游 fourslash *.ts  ──convertFourslash.mts──▶  internal/fourslash/tests/gen/*_test.go
  (//// 内容 = 测试输入)        (parse verify.xxx / goTo.xxx)        (Go f.Verify* / f.GoTo* 调用)
```

`convertFourslash.mts` 的逻辑（已读）：
- `getTestInput`：抽 `////` 开头的行作为带 marker 的源码、`// @` 作为指令；其余丢弃。
- `parseFourslashStatement`：把 TS 的 `verify.completions(...)` / `goTo.marker(...)` / `verify.baselineFindAllReferences()` 等映射成 Go 命令；不支持的命令 → 该文件进 `unparsedTests.txt`（2603 个未能转换）。
- `manualTests.txt`（172）：手工维护、不自动生成的；`tests/manual/` 对应。
- `allowedCodeFixIds` / `allowedCodeFixDescriptionPrefixes`：只转换已实现的 code fix。

**Rust 侧的正确做法（三选一，推荐 A）：**

- **方案 A（推荐）——改生成器输出模板**：保留 `convertFourslash.mts` 的解析逻辑（它解析 TS 上游、抽 fixture、识别命令），只把它的**输出后端**从"Go 测试"改成"Rust 测试"（`f.Verify*` → `f.verify_*`，import 换 crate 路径，content 字符串转义）。这样 4250 个 Rust 测试由**同一批 TS fixture 重新生成**，与 Go 始终同源。生成器是工具脚本，不是被移植的"业务代码"，允许用 TS/Node 实现。
- **方案 B——以现有 `.go` 生成产物为中间表示**：写一个把 `tests/gen/*_test.go` 的 `f.Verify*(...)` 调用序列转成等价 Rust 调用的转换器（纯机械翻译命令序列，不翻译框架）。比 A 多一层，但不依赖 TS 上游。
- **方案 C（不推荐）——手写**：仅对 `tests/manual/`（205）这种本就手工的少量测试手写 Rust 版；其余一律生成。

**无论哪种方案，不变量：**
- **fixture 共享**：`////` 源码 + `[|range|]` + `/*marker*/` 来自 TS 上游，Rust runner 直接解析同样的字符串（`parse_test_data`），不复制翻译 fixture。
- **baseline 共享**：`testdata/baselines/reference/fourslash/**` 是 ground truth，Rust 跑出的 baseline 与它逐字节比（`baseline::run`）。
- **跳过名单共享**：`failingTests.txt` / `crashingTests.txt` 原样读。
- **内联 expected 共享**：completion item 列表等来自 `tests/util` 的常量（已 1:1 移植）+ 生成器原样搬运的字面量。

## 抽样代表性测试（说明对齐方式，非全量）

以下 3-5 个代表性 `*_test.go` 展示**生成测试长什么样 + Rust 等价怎么写**，覆盖三大命令风格（内联 completion / baseline goto / baseline find-refs）。这些是"对齐方式"示范，不是要逐个枚举 4250 个。

### 样本 1：completion（内联断言）— `tests/gen/asOperatorCompletion_test.go`

Go：
```go
func TestAsOperatorCompletion(t *testing.T) {
	fourslash.SkipIfFailing(t)
	t.Parallel()
	defer testutil.RecoverAndFail(t, "Panic on fourslash test")
	const content = `type T = number;
var x;
var y = x as /**/`
	f, done := fourslash.NewFourslash(t, nil, content)
	defer done()
	f.VerifyCompletions(t, "", &fourslash.CompletionsExpectedList{
		IsIncomplete: false,
		ItemDefaults: &fourslash.CompletionsExpectedItemDefaults{
			CommitCharacters: &DefaultCommitCharacters, EditRange: Ignored,
		},
		Items: &fourslash.CompletionsExpectedItems{ Includes: []fourslash.CompletionsExpectedItem{"T"} },
	})
}
```
Rust 等价（生成）：
```rust
#[test]
fn test_as_operator_completion() {
    let mut h = Harness::new();
    if h.skip_if_failing() { return; }
    let content = "type T = number;\nvar x;\nvar y = x as /**/";
    let (mut f, done) = FourslashTest::new(&mut h, None, content);
    f.verify_completions(&mut h, "", &CompletionsExpectedList {
        is_incomplete: false,
        item_defaults: Some(CompletionsExpectedItemDefaults {
            commit_characters: Some(DEFAULT_COMMIT_CHARACTERS.clone()), edit_range: Ignored,
        }),
        items: Some(CompletionsExpectedItems { includes: vec!["T".into()], ..Default::default() }),
    });
    done(&mut h);
}
```
对齐点：`content`（含 `/**/` marker）逐字节相同；`Includes: ["T"]` 字面量相同；`DefaultCommitCharacters` 来自 `tests/util`。**断言是内联的，不写 baseline 文件。**

### 样本 2：go-to-definition（baseline + 内联混合）— `tests/gen/ambientShorthandGotoDefinition_test.go`

Go 关键行：
```go
const content = `// @Filename: declarations.d.ts
declare module /*module*/"jquery"
// @Filename: user.ts
///<reference path="declarations.d.ts"/>
import [|/*importFoo*/foo|], {bar} from "jquery";
...`
f.VerifyQuickInfoAt(t, "useFoo", "(alias) module \"jquery\"\nimport foo", "")
f.VerifyBaselineGoToDefinition(t, true, "useFoo", "importFoo", "useBar", ...)
```
对齐点：① 多文件（`@Filename`）+ range 嵌 marker（`[|/*importFoo*/foo|]`）；② `VerifyQuickInfoAt` 是内联（expected quickinfo 文本逐字节）；③ `VerifyBaselineGoToDefinition` 写 `reference/fourslash/goToDefinition/ambientShorthandGotoDefinition.baseline.jsonc`，Rust 跑出后逐字节比。

### 样本 3：find-all-references（纯 baseline）— `tests/gen/constructorFindAllReferences1_test.go`

Go：
```go
const content = `export class C {
    /**/public constructor() { }
    public foo() { }
}
new C().foo();`
f.VerifyBaselineFindAllReferences(t, "")
```
对齐点：单 marker `/**/`；`VerifyBaselineFindAllReferences` 把 find-refs 响应渲染进 `findAllReferences` baseline 缓冲 → 结束对拍 `reference/fourslash/findAllReferences/constructorFindAllReferences1.baseline.jsonc`。**整条断言就是 baseline 对拍**——这是 fourslash 最常见的形态，也是为什么 P10 是"端到端对拍收口 phase"。

### 样本 4（建议补）：rename / hover —— 同 baseline 模式，验证 `VerifyBaselineRename` 的 submodule `DiffFixupOld`（preference 改名 + context span `<| |>` 剥离）路径。

### 样本 5（建议补）：`tests/manual/` 里一个手工测试 —— 验证手写路径（方案 C）与生成路径并存。

## TDD 推进顺序（tracer bullet → 增量）

1. **`test_parser::parse_file_content`**：DSL 状态机最独立、最易测。给 `[|/*m*/foo|]` 等输入，断言抠出的纯内容 + marker.position + range。这是一切的地基。
2. **`tests/util` 常量表 + `sortCompletionItems`**：completion 内联断言全靠它。
3. **`new_fourslash` + `lsptestutil` server 起停**（tracer bullet）：跑样本 1（completion）打穿"解析→起 server→发 completion 请求→内联断言"。
4. **baseline 路径**：跑样本 3（find-refs）打穿"渲染 baseline → `baseline::run` 对拍 reference"。
5. **生成器**（方案 A）：改 `convertFourslash.mts` 输出 Rust，先生成一小批（如 completion 族 + goto 族），分批跑、分批进 `failingTests.txt`。
6. 按命令族逐步开绿（completion → goto/find-refs → hover/signature → rename → 其余）；每开一族就把对应已通过的测试从 `failingTests.txt` 移除。

## 与 Go 的已知偏离（divergence）

1. **4250 测试不翻译**：用生成器复用 fixture（见上）。这是与 Bun"逐文件翻译"方法论的**有意偏离**，因为 Go 测试本身就是生成物。
2. **`*testing.T` / `t.Parallel` / `t.Run`** → `&Harness`（同 testutil）。fourslash 每个测试是独立 `#[test]`，内部 `done()` 触发 baseline 对拍。
3. **`runtime.Caller(1)` 拿 testPath** → 生成器在每个测试里显式传 `file!()` 或测试源路径（submodule diff fixup 需要）。
4. **内存 LSP 往返**：Go 用 channel + goroutine server。Rust 用 `lsptestutil` 的内存 reader/writer + `std::thread`（PORTING §6），保证请求/响应顺序确定。
5. **`failingTests.txt` 是活动状态**：移植期它会很大（先全 skip），随命令族开绿逐步缩小——这是**预期的、受控的"未完成"**，不违反"编译通过+测试全绿"（skip 的测试不算红）。
6. **baseline 扩展名/子目录** 必须与 Go 完全一致（`.baseline.jsonc` / `fourslash/<cmd>/` 等），否则与 reference 路径对不上。

## 转交 / 推迟（DEFER）

- `statebaseline`（`@statebaseline` 模式）：仅少量测试用，可在主命令族开绿后补。`// DEFER(phase-10): blocked-by: 主命令族 parity`
- semantic tokens / call hierarchy / inlay hints 等"长尾命令"：按 `failingTests.txt` 分批，靠后。
- code fix 命令族：仅 `allowedCodeFixIds`（fixMissingImport 等 3 个）+ allowed 描述前缀的子集被生成；其余在 `tests/util` / 生成器侧已被过滤，无需移植。
- 生成器本体（`convertFourslash.mts`）：可继续用 TS/Node（工具脚本，不是被移植的库代码）；只改输出模板。
