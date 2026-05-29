# ls: 实现方案（impl.md）

> 写之前已实际通读 `internal/ls/` 全部 60 个非测试 `.go`（根 36 + `lsconv` 2 + `lsutil` 8 + `change` 3 + `autoimport` 11）。文件用真实名 + 主函数签名 + `// Go:` 锚。本包体量大，impl.md 按**子系统**分节，但覆盖全部 60 文件。

**crate（族）**：`tsgo_ls`（根）+ 4 个子 crate（见下「crate 拆分」）　**目标**：实现 TypeScript **语言服务核心**——补全 / 悬停 / 跳转定义 / 查找引用 / 重命名 / 签名帮助 / 语义高亮 / inlay hints / 折叠 / 文档符号 / 调用层级 / code action / organize imports / 文档诊断 / 格式化转发等，把内部计算结果转成 **LSP 协议类型**（`lsproto`）。
**Go 源**：`internal/ls/`（60 个非测试文件；`completions.go` 215KB、`findallreferences.go` 99KB、`string_completions.go` 72KB、`signaturehelp.go` 54KB、`utilities.go` 52KB、`autoimport/registry.go` 68KB 是体量大头）

## 这个包是什么（业务说明）

`tsgo_ls` 是「编译器内核（checker/program）」与「LSP 服务器（P8 `lsp`/`project`）」之间的**语言服务层**。每个特性的形态高度一致：`LanguageService.Provide<Feature>(ctx, params) -> (lsproto.<Feature>Response, error)`——
1. 用 `lsconv.Converters` 把 LSP 的 `(line, character)`（**UTF-16** 码元）换算成内部 UTF-8 字节 `position`；
2. 用 `astnav` 在 `ast.SourceFile` 上定位 token / node；
3. 按需取 `program.GetTypeCheckerForFile(ctx, file)`（带 `done()` 释放）做语义查询；
4. 计算结果（节点、符号、文本编辑…）→ 用 `Converters` 把内部 `position`/`TextRange` 换回 LSP `Range`，组装 `lsproto` 响应。

下游关系：P8 `lsp` 把 JSON-RPC 请求解码后调本层 `Provide*`；P8 `project` 提供 `Host`（`ReadFile`/`Converters`/`AutoImportRegistry`/`GetPreferences`…）与跨工程编排（`CrossProjectOrchestrator`）。`fourslash`（4250 用例）在 **P10** 对本层做端到端 parity——所以本层**绝大多数特性的细粒度正确性由 P10 兜底**，本 phase 的单测只有 8 个文件 21 个 `func`（见 `tests.md`）。

为什么在 P7：依赖 P1–P6 的全部内核（`ast`/`scanner`/`astnav`/`checker`/`compiler`/`printer`/`module`/`modulespecifiers`/`sourcemap`/`nodebuilder` …）+ 同 phase 的 `format`；被 P8 使用。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包特有：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `LanguageService{ projectPath, host Host, activeConfig, program *compiler.Program, converters *lsconv.Converters, documentPositionMappers map }` | `struct LanguageService<'p>`：持 `Arc<Program>` / `&Program`、`Box<dyn Host>`、`UserPreferences`、`Converters`、`FxHashMap<String, DocumentPositionMapper>` | 一次请求生命周期；`documentPositionMappers` 是请求内缓存 |
| `Host interface`（11 方法） | `trait Host`（`use_case_sensitive_file_names`/`read_file`/`converters`/`get_preferences`/`get_ecma_line_info`/`auto_import_registry`/`read_directory`/`get_directories`/`directory_exists`/`file_exists`） | P8 `project` 实现 |
| `program.GetTypeCheckerForFile(ctx, file) (c, done)` | `let (c, done) = program.get_type_checker_for_file(ctx, file); ` + `defer done()` → RAII guard（`scopeguard`/`Drop`） | checker 是池化租借，必须释放 |
| `*ast.Node` / `*ast.Symbol` / `*checker.Type` | `NodeId` 索引（AST）/ `&Symbol`（checker arena 句柄）/ `&Type` | 见 PORTING §5；symbol/type 来自 checker，本层只借用 |
| 各 `lsproto.<X>Response`（联合：`LocationOrLocationsOrDefinitionLinksOrNull` 等） | `enum`（判别联合）或 `serde(untagged)`——由 P8 `lsproto` crate 定义，本层只构造 | 协议类型归 P8（`tsgo_lsp_lsproto`）；本层 `path` 依赖它 |
| `context.Context`（取消 + ClientCapabilities + locale + UserPreferences + FormatSettings） | `&Cancel`（`Arc<AtomicBool>`，PORTING §3）+ 显式 `&ClientCapabilities` / `&Locale`；ClientCaps 现状从 ctx 取（`lsproto.GetClientCapabilities(ctx)`）→ Rust 改显式传参 | 偏离：不复刻 context value |
| `collections.SyncMap` / `SyncSet`（crossproject） | `dashmap::DashMap` / `Mutex<HashSet>` | 跨工程并发收集 |
| `core.NewWorkGroup(false)`（crossproject 并行队列） | `rayon` scope 或 `std::thread::scope` + `crossbeam-channel`（PORTING §6） | **真并行**，结果按稳定 key 排序保确定性 |

### 命门：UTF-16 ↔ UTF-8 位置换算（`lsconv`）

LSP 协议里位置是 **0-based (line, character)**，`character` 默认按 **UTF-16 码元**计（除非协商 UTF-8/UTF-32）；而内部一切偏移是 **UTF-8 字节**。换算集中在 `lsconv/converters.go` + `lsconv/linemap.go`，是全 ls 的地基（每个 `Provide*` 都用）。要点（Rust 移植须 1:1）：

- `LSPLineMap{ LineStarts []TextPos, AsciiOnly bool }`：`ComputeLSPLineStarts` 只认 `\n`/`\r`/`\r\n` 为换行（与 `core.ComputeLineStarts` 的 Unicode 行分隔符不同），并记录整文件是否纯 ASCII（快路径）。
- `LineAndCharacterToPosition`（LSP→字节）：clamp line；若 `AsciiOnly || 编码==UTF8` 直接 `start+char`；否则从行首用 **`utf8::decode_rune_in_string`** 逐字符累加 `utf16::rune_len(r)`，直到达到 `char`。**关键**：用 `DecodeRuneInString` 而非 `for range`+`RuneLen`，使非法 UTF-8 字节按实际 1 字节前进（而非 `RuneLen(RuneError)==3`）——`TestConvertersInvalidUTF8` 专测此。Rust 侧用 `str::char_indices` 对合法 UTF-8 等价，但**非法字节**需用按字节解码（`bstr` 或手写 `utf8::decode`），保证 RuneError 前进 1 字节、UTF-16 长度 1。**这是 P7 待定项**（crate-map.md「待定/UTF-16」），建议自实现一个 `decode_utf8_rune(&[u8]) -> (char, usize)` 镜像 Go 的 `utf8.DecodeRuneInString` 语义（含 RuneError）。
- `PositionToLineAndCharacter`（字节→LSP）：clamp；`slices.BinarySearch` 找行；若非 ASCII 用 `for r in text[start..position]` 累加 `utf16::rune_len`。
- Rust crate 选型：`std::char` + 自写 utf16 长度（BMP=1, 非 BMP=2，即 `c as u32 > 0xFFFF ? 2 : 1`，对应 `utf16::RuneLen`）；非法字节按 `char::REPLACEMENT_CHARACTER` 长度 1 处理。**不**用 `widestring`（无需真转 UTF-16 缓冲）。

### 并发点

- `crossproject.go::handleCrossProject`：用 `core.NewWorkGroup` + `SyncMap` 跨工程并行搜索（references/rename/implementations/callhierarchy）→ Rust `rayon`/scoped threads；结果迭代器 `getResultsIterator` 有严格顺序（default project 优先、再 all projects、再 originalLocation），**必须保序**以保证 `combine*` 输出确定。
- `findallreferences.go` / `documenthighlights.go` 多文件搜索：Go 顺序遍历 `sourceFiles`；可 `par_iter` 但收集后须按文件稳定排序。
- `autoimport/registry.go`：`createCheckerPool`（`util.go`）池化 checker，`registryBuilder` 并发抽取 export（`extractPackage`）→ `rayon`，但索引写入需同步。

## crate 拆分（4 子目录 → 子 crate）

Go 里 `internal/ls` 的 4 个子目录各是独立 package。Rust 侧**各拆一个 crate**（`tsgo_ls_<name>`），原因与依赖序：

```
tsgo_ls_lsconv   (叶：位置换算 + 诊断转 LSP)   依赖: ast core diagnostics diagnosticwriter locale lsproto tspath bundled collections
tsgo_ls_lsutil   (叶：LS 通用工具 + 偏好)      依赖: ast astnav checker compiler core scanner stringutil tspath json modulespecifiers vfsmatch printer lsproto
tsgo_format                                    依赖: tsgo_ls_lsutil (+ ast astnav core scanner ...)   ← P7 同 phase
tsgo_ls_change   (LSP 文本编辑追踪器)          依赖: tsgo_ls_lsutil tsgo_ls_lsconv tsgo_format ast astnav core scanner lsproto
tsgo_ls_autoimport (自动导入注册表/索引/修复)  依赖: tsgo_ls_lsutil tsgo_ls_lsconv tsgo_ls_change tsgo_format ast checker compiler core module modulespecifiers packagejson symlinks vfs lsproto logging
tsgo_ls          (根：36 文件，全部 feature)   依赖: 以上全部 + checker compiler printer nodebuilder sourcemap outputpaths module modulespecifiers packagejson tsoptions binder ...
```

> **存疑偏离**：PORTING §2 默认「嵌套子包作父 crate 的子 module」。此处必须拆 crate——`tsgo_format` 依赖 `lsutil`，若 `lsutil` 是 `tsgo_ls` 的子 module 则 `format→ls→format` 成环（Cargo 禁止）。为统一与 1:1 映射 Go package，把 4 个子目录全部拆成 `tsgo_ls_<name>` crate。建议把这 5 个 crate 名（`tsgo_ls`/`tsgo_ls_lsconv`/`tsgo_ls_lsutil`/`tsgo_ls_change`/`tsgo_ls_autoimport`）记入根 README crate 清单。
> 文件命名（PORTING §2）：每个子 crate 根文件 basename==dir 名时用 `lib.rs`（如 `lsconv/converters.go`≠dir 名 `lsconv` → `lib.rs` 取哪个？dir 内无与 dir 同名文件，故选一个作 `lib.rs`，建议 `converters.go→lib.rs`；其余 `linemap.go→linemap.rs`）。`tsgo_ls` 根 36 文件择 `languageservice.go→lib.rs`，其余同名 `.rs`。

## 依赖白名单（本包新增的 crate）

- `dashmap`（PORTING §10 已列）：crossproject `SyncMap/SyncSet`、autoimport 并发。
- `rayon` / `crossbeam-channel`（已列）：crossproject 并行、autoimport 抽取池。
- 自实现 `decode_utf8_rune`（无新 crate）：镜像 `utf8.DecodeRuneInString` 含 RuneError 语义（见 UTF-16 节）；若选第三方则 `bstr`，须记入 crate-map.md。
- `serde`/`serde_json`（已列）：`userpreferences` 的 `MarshalJSONTo`/`UnmarshalJSONFrom`（见 lsutil 节）。
- 反射替代：Go `userpreferences.go` 用 `reflect` 做 tag 驱动的字段映射；Rust 无运行时反射 → 见 lsutil 节的「偏离」。

---

# 文件清单 → Rust 模块（按子系统，覆盖全部 60 文件）

## 子系统 A · 基础设施 / 服务入口（9 文件，`tsgo_ls` 根）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `languageservice.go` | `lib.rs` | `LanguageService` 结构 + `NewLanguageService` + `toPath`/`GetProgram`/`UserPreferences`/`FormatOptions`/`tryGetProgramAndFile`/`getProgramAndFile`/`GetDocumentPositionMapper`/`ReadFile`/`UseCaseSensitiveFileNames`/`GetECMALineInfo`/`getPreparedAutoImportView`/`getCurrentAutoImportView`/`DirectoryExists`/`ReadDirectory`/`GetDirectories` |
| `host.go` | `host.rs` | `trait Host`（11 方法） |
| `api.go` | `api.rs` | `GetSymbolAtPosition`/`GetSymbolAtLocation`/`GetTypeOfSymbol` + `ErrNoSourceFile`/`ErrNoTokenAtPosition` |
| `constants.go` | `constants.rs` | `moduleSpecifierResolutionLimit=100`/`...CacheAttemptLimit=1000` |
| `diagnostics.go` | `diagnostics.rs` | `ProvideDiagnostics`/`getAllDiagnostics`/`toLSPDiagnostics`（syntactic+semantic+suggestion+declaration） |
| `utilities.go` | `utilities.rs` | ~80 个共享工具（见下 TODO）：`createLspRangeFrom*`/`getMeaningFromLocation`/`getAdjustedLocation`/`getReferenceAtPosition`/`getContainingObjectLiteralElement`/`getPossibleTypeArgumentsInfo`/… |
| `displaypartswriter.go` | `displaypartswriter.rs` | `displayPartsWriter`（实现 printer 的 `EmitTextWriter`，把符号显示拆成分类文本片段供 hover/completion 详情） |
| `source_map.go` | `source_map.rs` | `getMappedLocation`/`tryGetSourcePosition`/`tryGetGeneratedPosition`（.d.ts ↔ 源 sourcemap 跳转）+ `script` 适配器 |
| `crossproject.go` | `crossproject.rs` | `CrossProjectOrchestrator` trait + `handleCrossProject`（并行跨工程）+ `combineReferences/VsReferences/Implementations/RenameResponse/IncomingCalls`/`combineLocationArray` |

## 子系统 B · 位置换算 `lsconv`（2 文件，crate `tsgo_ls_lsconv`）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `lsconv/converters.go` | `lsconv/lib.rs` | `Converters{ getLineMap, positionEncoding }` + `ToLSPRange`/`FromLSPRange`/`FromLSPTextChange`/`ToLSPLocation`/`LineAndCharacterToPosition`/`PositionToLineAndCharacter`（UTF-16 核心）+ `FileNameToDocumentURI`/`DocumentUri.FileName()`（URI↔路径）+ `DiagnosticToLSPPull`/`DiagnosticToLSPPush`/`diagnosticToLSP` + `LanguageKindToScriptKind` |
| `lsconv/linemap.go` | `lsconv/linemap.rs` | `LSPLineMap{ LineStarts, AsciiOnly }` + `ComputeLSPLineStarts`（只认 \n/\r/\r\n）+ `ComputeIndexOfLineStart`（二分） |

## 子系统 C · LS 通用工具/偏好 `lsutil`（8 文件，crate `tsgo_ls_lsutil`）

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `lsutil/userpreferences.go` | `lsutil/userpreferences.rs` | `UserPreferences`（巨结构，tag 驱动）+ `FormatCodeSettings`?（在 formatcodeoptions）+ `NewDefaultUserPreferences`/`withConfig`/`MarshalJSONTo`/`UnmarshalJSONFrom`/`WithOverrides`/`ParseUserPreferences`/`ModuleSpecifierPreferences`/`IsATADisabled`；枚举 `QuotePreference`/`JsxAttributeCompletionStyle`/`OrganizeImports*` 等 |
| `lsutil/formatcodeoptions.go` | `lsutil/formatcodeoptions.rs` | `EditorSettings`/`FormatCodeSettings`/`IndentStyle`/`SemicolonPreference` + `GetDefaultFormatCodeSettings`/`FromLSFormatOptions`/`ToLSFormatOptions`/`parseIndentStyle`/`parseSemicolonPreference` |
| `lsutil/asi.go` | `lsutil/asi.rs` | 自动分号插入候选判定：`PositionIsASICandidate`/`NodeIsASICandidate`/`SyntaxRequiresTrailing*`/`SyntaxMayBeASICandidate` |
| `lsutil/children.go` | `lsutil/children.rs` | `GetLastChild`/`GetLastToken`/`GetLastVisitedChild`/`GetFirstToken`/`AssertHasRealPosition`（scanner 重扫拿未访问 token） |
| `lsutil/completednode.go` | `lsutil/completednode.rs` | `PositionBelongsToNode`/`IsCompletedNode`（节点是否「闭合」）/`nodeEndsWith`/`hasChildOfKind` |
| `lsutil/organizeimports.go` | `lsutil/organizeimports.rs` | 导入排序/比较器：`GetDetectionLists`/`getOrganizeImports{Ordinal,Unicode}StringComparer`/`CompareImportsOrRequireStatements`/`GetNamedImportSpecifierComparer*`/`DetectNamedImportOrganizationBySort`/`measureSortedness` 等 |
| `lsutil/symbol_display.go` | `lsutil/symbol_display.rs` | `GetSymbolKind`/`GetSymbolModifiers`/`ScriptElementKind`/`ScriptElementKindModifier`（符号→脚本元素种类/修饰符，供 completion/symbols/hover） |
| `lsutil/utilities.go` | `lsutil/utilities.rs` | `ProbablyUsesSemicolons`/`ShouldUseUriStyleNodeCoreModules`/`GetQuotePreference`/`QuotePreferenceFromString`/`ModuleSpecifierToValidIdentifier`/`ModuleSymbolToValidIdentifier`/`IsNonContextualKeyword` |

## 子系统 D · 文本编辑追踪 `change`（3 文件，crate `tsgo_ls_change`）

> 对应 strada `services/textChanges.ts`：累积「替换/插入/删除节点」的高层编辑，最后用 `format` 重排缩进/空白，产出 `lsproto.TextEdit`。是所有 code action / organize imports / file rename 的写出层。

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `change/tracker.go` | `change/lib.rs` | `Tracker`/`NodeOptions`/`trackerEdit` + `NewTracker`/`GetChanges` + 编辑 API：`ReplaceNode(s)`/`ReplaceRange*`/`InsertText`/`InsertNodeAt(s)`/`InsertNodeAfter/Before`/`InsertNodeInListAfter`/`InsertImportSpecifierAtIndex`/`InsertAtTopOfFile`/`InsertMemberAtStart`/`Delete`/`DeleteRange`/`DeleteNode(Range)`/`TryInsertTypeAnnotation`/`ParenthesizeArrowParameters`/`InsertModifierBefore` + 缩进计算 |
| `change/trackerimpl.go` | `change/trackerimpl.rs` | 输出实现：`getTextChangesFromChanges`/`computeNewText`/`getFormattedTextOfNode`（调 `format`）/`getNonformattedText`/`GetAdjustedRange`/`getAdjustedStartPosition`/`getAdjustedEndPosition`/`getInsertionPositionAtSourceFileTop` |
| `change/delete.go` | `change/delete.rs` | 删除细节：`deleteDeclaration`/`deleteDefaultImport`/`deleteImportBinding`/`deleteVariableDeclaration`/`deleteNode(InList)`/`startPositionToDeleteNodeInList`/`endPositionToDeleteNodeInList` |

## 子系统 E · 自动导入 `autoimport`（11 文件，crate `tsgo_ls_autoimport`）

> 宿主侧**增量索引**（`Registry`，跨请求存活、随 program 变更 `Clone` 更新）+ 请求侧**视图**（`View`，按当前导入文件解析模块说明符）+ **修复生成**（`Fix`/`ImportAdder`，产出导入语句编辑）。registry 是本 phase 唯一有较多直接单测的子系统（见 tests.md）。

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `autoimport/registry.go` | `autoimport/lib.rs` | `Registry`/`RegistryBucket`/`BucketState`/`directory`/`RegistryChange`/`RegistryCloneHost` + `NewRegistry`/`IsPreparedForImportingFile`/`Clone`/`NodeModulesDirectories`/`GetCacheStats` + `registryBuilder`（增量重建：`updateBucketAndDirectoryExistence`/`markBucketsDirty`/`updateIndexes`/`buildProjectBucket`/`buildNodeModulesBucket`/`discoverBucketPackages`/`extractPackage`） |
| `autoimport/view.go` | `autoimport/view.rs` | `View` + `NewView`/`Search`/`SearchByExportID`/`GetCompletions`/`getAllowedEndings`；`FixAndExport` |
| `autoimport/fix.go` | `autoimport/fix.rs` | `Fix`/`newImportBinding`/`addToExistingImportFix` + `Edits`/`GetFixes`/`tryAddToExistingImport`/`tryUseExistingNamespaceImport`/`getNewImports`/`getNewRequires`/`getImportKind`/`getExistingImports`/`shouldUseRequire`/`detectSyntax`/`promoteFromTypeOnly`/`CompareFixesFor{Sorting,Ranking}`/`insertImports`/`makeImport` |
| `autoimport/import_adder.go` | `autoimport/import_adder.rs` | `trait ImportAdder` + `importAdder` 实现 + `NewImportAdder`/`AddImportFromExportedSymbol`/`AddImportFix`/`Edits`/`HasFixes` + `TypeToAutoImportableTypeNode`/`TryGetAutoImportableReferenceFromTypeNode`/`getNameForExportedSymbol`/`importSymbols` |
| `autoimport/export.go` | `autoimport/export.rs` | `ModuleID`/`ExportID`/`ExportSyntax`(10 值)/`Export` + `Name`/`IsRenameable`/`AmbientModuleName`/`IsUnresolvedAlias`/`SymbolToExport`/`tryGetModuleExport`/`extractFirstExport` |
| `autoimport/export_stringer_generated.go` | `autoimport/export_stringer_generated.rs` | `ExportSyntax::String()`（生成代码）→ Rust 用 `#[derive(strum::Display)]` 或手写 `Display`（无需复刻 stringer 机制） |
| `autoimport/extract.go` | `autoimport/extract.rs` | `symbolExtractor`/`exportExtractor`/`checkerLease` + `extractFromFile/Module/Symbol`/`createExport`/`tryResolveSymbol`/`getSyntax`/`shouldIgnoreSymbol`/`isUnusableName` |
| `autoimport/index.go` | `autoimport/index.rs` | 泛型 `Index[T: Named]`（首字母倒排）+ `Find`/`SearchWordPrefix`/`insertAsWords`/`Clone`/`containsCharsInOrder` |
| `autoimport/util.go` | `autoimport/util.rs` | `wordIndices`（camelCase 分词）/`getPackageRealpathFuncs`（符号链接 realpath）/`createCheckerPool`/`getResolvedPackageNames`/`getPackageNamesInNodeModules`/`addPackageJsonDependencies`/`getModuleResolver`/`resolutionHost` |
| `autoimport/aliasResolver.go`（`aliasresolver.go`） | `autoimport/aliasresolver.rs` | `aliasResolver`（实现 module resolver 所需的 ~40 个 host 方法）+ `newAliasResolver`/`BindSourceFiles` |
| `autoimport/specifiers.go` | `autoimport/specifiers.rs` | `View.GetModuleSpecifier`（算从导入文件到目标的模块说明符） |

## 子系统 F · 导航类特性（8 文件，`tsgo_ls` 根）

| Go 文件 | Rust 文件 | 主入口 |
|---|---|---|
| `definition.go` | `definition.rs` | `ProvideDefinition`/`provideDefinitionWorker`/`ProvideTypeDefinition` + `createDefinitionLocations`/`getDeclarationsFromLocation`/`tryGetSignatureDeclaration` |
| `sourcedefinition.go` | `sourcedefinition.rs` | `ProvideSourceDefinition`（.d.ts→源实现跳转）+ `sourceDefResolver`（解析/搜索实现文件） |
| `findallreferences.go` | `findallreferences.rs` | `ProvideReferences`/`ProvideVsReferences`/`ProvideImplementations`/`provideSymbolsAndEntries`（查找引用核心引擎；`refOptions`/`SymbolAndEntries`/`DefinitionKind`…） |
| `documenthighlights.go` | `documenthighlights.rs` | `ProvideDocumentHighlights`/`ProvideMultiDocumentHighlights` + 语义高亮 + 语法高亮（if/else、try/catch、break/continue、return、throw、modifier 等占用） |
| `callhierarchy.go` | `callhierarchy.rs` | `ProvidePrepareCallHierarchy`/`ProvideCallHierarchy{Incoming,Outgoing}Calls` + `resolveCallHierarchyDeclaration`/`collectCallSites` |
| `importTracker.go` | `importTracker.rs` | `createImportTracker`/`getImportersForExport`/`getImportOrExportSymbol`/`findModuleReferences`（references 引擎的导入追踪子系统） |
| `rename.go` | `rename.rs` | `ProvideRename`/`GetRenameInfo`/`symbolAndEntriesToRename`/`getRenameInfoForNode`/`getTextForRename`/`nodeIsEligibleForRename` |
| `file_rename.go` | `file_rename.rs` | `GetEditsForFileRename`（文件改名→更新 import/tsconfig paths）+ `updateImportsForFileRename`/`updateTsconfigFiles` |

## 子系统 G · 信息/呈现类特性（12 文件，`tsgo_ls` 根）

| Go 文件 | Rust 文件 | 主入口 |
|---|---|---|
| `hover.go` | `hover.rs` | `ProvideHover`（quickInfo：符号类型 + JSDoc + 可展开 verbosity） |
| `completions.go` | `completions.rs` | `ProvideCompletion`/`ResolveCompletionItem` + `getCompletionsAtPosition`（**最大文件**；符号补全/关键字/JSX/auto-import/class member snippets…） |
| `string_completions.go` | `string_completions.rs` | `getStringLiteralCompletions`（字符串字面量内补全：模块路径/字面量联合类型/属性名） |
| `signaturehelp.go` | `signaturehelp.rs` | `ProvideSignatureHelp`/`GetSignatureHelpItems`（参数提示；`callInvocation`/`typeArgsInvocation`/`contextualInvocation`） |
| `symbols.go` | `symbols.rs` | `ProvideDocumentSymbols`/`ProvideWorkspaceSymbols`（文档/工作区符号树 + expando 合并 + 模糊匹配评分） |
| `semantictokens.go` | `semantictokens.rs` | `ProvideSemanticTokens(Range)`/`SemanticTokensLegend`/`collectSemanticTokens`/`classifySymbol`/`encodeSemanticTokens`（LSP 增量编码） |
| `inlay_hints.go` | `inlay_hints.rs` | `ProvideInlayHint` + `inlayHintState`（参数名/类型/返回类型/枚举值 hint） |
| `folding.go` | `folding.rs` | `ProvideFoldingRange`（节点折叠 + `#region`/`#endregion` + 注释折叠） |
| `selectionranges.go` | `selectionranges.rs` | `ProvideSelectionRanges`/`getSmartSelectionRange`（智能选区扩展） |
| `linkedediting.go` | `linkedediting.rs` | `ProvideLinkedEditingRange`（JSX 开/闭标签联动编辑） |
| `autoinsert.go` | `autoinsert.rs` | `ProvideOnAutoInsert`（输入 `>` 自动补 JSX 闭合标签/fragment） |
| `codelens.go` | `codelens.rs` | `ProvideCodeLenses`/`ResolveCodeLens`（references/implementations code lens） |

## 子系统 H · 编辑/重构类特性（6 文件，`tsgo_ls` 根）

| Go 文件 | Rust 文件 | 主入口 |
|---|---|---|
| `format.go` | `format.rs` | `ProvideFormatDocument(/Range/OnType)` + `getFormattingEditsFor{Document,Range}`/`getFormattingEditsAfterKeystroke`（转发到 `tsgo_format`）+ `getRangeOfEnclosingComment` |
| `organizeimports.go` | `organizeimports.rs` | `OrganizeImports`/`organizeImportsWorker`/`coalesceImportsWorker`/`coalesceExportsWorker`/`removeUnusedImports`/`groupBy*`（整理导入） |
| `codeactions.go` | `codeactions.rs` | `CodeFixProvider`/`CodeFixContext`/`CodeAction`/`CombinedCodeActions` + `ProvideCodeActions`/`getFixAllQuickFixes`/`createOrganizeImportsAction`/`convertToLSPCodeAction` + `codeFixProviders` 注册表 |
| `codeactions_importfixes.go` | `codeactions_importfixes.rs` | `ImportFixProvider`：`getImportCodeActions`/`getAllImportCodeActions`/`getFixInfos`/`getFixesInfoFor{UMD,NonUMD}Import`/`getTypeOnlyPromotionFix`（「找不到名字 → 自动 import」修复） |
| `codeactions_fixmissingtypeannotation.go` | `codeactions_fixmissingtypeannotation.rs` | `IsolatedDeclarationsFixProvider`：`isolatedDeclarationsFixer`（isolatedDeclarations 缺类型注解修复：加注解/内联断言/提取变量） |
| `codeactions_fixclassincorrectlyimplementsinterface.go` | `codeactions_fixclassincorrectlyimplementsinterface.rs` | `FixClassIncorrectlyImplementsInterfaceProvider`：补全 class 缺失的 interface 成员（用 `missingMemberFixer`） |
| `codeactions_missingmemberfixer.go` | `codeactions_missingmemberfixer.rs` | `missingMemberFixer`（共享：从符号/签名生成成员声明节点，供上面两个 fixer 复用） |

> 注：子系统 H 列 7 个文件，因 `codeactions_missingmemberfixer.go` 跨 fixer 复用单列。H 实际 7 文件，F=8、G=12、A=9、B=2、C=8、D=3、E=11 → **总计 60** ✓。

---

# 实现 TODO（逐文件 / 逐函数，可勾选）

> 因体量巨大，TODO 以「文件 → 主公开函数/类型」粒度列出（每个 `Provide*` 是一条），内部私有 helper 在实现时按 Go 同文件补齐。带 `// Go:` 锚。推进序见后文「TDD 推进顺序」。

## E1 lsconv（先行，全员依赖）

### `lsconv/linemap.rs`（Go: `internal/ls/lsconv/linemap.go`）
- [ ] `pub struct LSPLineMap { line_starts: Vec<TextPos>, ascii_only: bool }`　`// Go: linemap.go:LSPLineMap`
- [ ] `pub fn compute_lsp_line_starts(text: &str) -> LSPLineMap`（仅 \n/\r/\r\n；记录 ascii_only）　`// Go: linemap.go:ComputeLSPLineStarts`
- [ ] `pub fn compute_index_of_line_start(&self, target: TextPos) -> usize`（二分，未命中取前一行）　`// Go: linemap.go:ComputeIndexOfLineStart`

### `lsconv/lib.rs`（Go: `internal/ls/lsconv/converters.go`）
- [ ] `fn decode_utf8_rune(bytes: &[u8]) -> (char, usize)` — 镜像 `utf8.DecodeRuneInString`（非法→`(REPLACEMENT, 1)`）；`fn utf16_len(c: char) -> i32`（>0xFFFF→2 else 1）　`// Go: 基于 unicode/utf8 + utf16.RuneLen`
- [ ] `pub struct Converters { get_line_map, position_encoding }` + `NewConverters`　`// Go: converters.go:Converters/NewConverters`
- [ ] `pub fn line_and_character_to_position(&self, script, lc) -> TextPos`（**UTF-16→字节**，含非法字节按 1 前进）　`// Go: converters.go:LineAndCharacterToPosition`
- [ ] `pub fn position_to_line_and_character(&self, script, pos) -> Position`（字节→UTF-16）　`// Go: converters.go:PositionToLineAndCharacter`
- [ ] `to_lsp_range`/`from_lsp_range`/`from_lsp_text_change`/`to_lsp_location`　`// Go: converters.go:ToLSPRange...`
- [ ] `pub fn file_name_to_document_uri(file_name) -> DocumentUri` + `extraEscapeReplacer`（保留字 %XX）+ dynamic/bundled/volume 处理　`// Go: converters.go:FileNameToDocumentURI`
- [ ] `DocumentUri::file_name()`（反向；untitled/ts-nul-authority/volume 解码）— 注：Go 里 `FileName()` 在 lsproto，但 `converters_test.go` 测它 → 实现归 `tsgo_lsp_lsproto`（P8），本 crate 测试依赖之　`// Go: lsproto.DocumentUri.FileName`（**DEFER(phase-8): blocked-by tsgo_lsp_lsproto**）
- [ ] `language_kind_to_script_kind`　`// Go: converters.go:LanguageKindToScriptKind`
- [ ] `diagnostic_to_lsp_pull`/`diagnostic_to_lsp_push`/`diagnostic_to_lsp`/`message_chain_to_string`/`style_check_diagnostics` 集合　`// Go: converters.go:DiagnosticToLSP*`

## E2 lsutil

### `lsutil/formatcodeoptions.rs`（Go: `internal/ls/lsutil/formatcodeoptions.go`）
- [ ] `IndentStyle`/`SemicolonPreference` enum + `parse_indent_style`/`parse_semicolon_preference`　`// Go: formatcodeoptions.go:IndentStyle...`
- [ ] `pub struct EditorSettings` / `pub struct FormatCodeSettings`（含全部 insertSpace* / placeOpenBrace* 字段）　`// Go: formatcodeoptions.go:FormatCodeSettings`
- [ ] `get_default_format_code_settings`/`from_ls_format_options`/`to_ls_format_options`　`// Go: formatcodeoptions.go:GetDefaultFormatCodeSettings/FromLSFormatOptions/ToLSFormatOptions`

### `lsutil/userpreferences.rs`（Go: `internal/ls/lsutil/userpreferences.go`）
- [ ] `pub struct UserPreferences{...}`（全字段）+ 子结构 `InlayHintsPreferences`/`CodeLensUserPreferences` + 枚举 `QuotePreference`/`JsxAttributeCompletionStyle`/`IncludeInlayParameterNameHints`/`OrganizeImportsCollation/CaseFirst/TypeOrder`　`// Go: userpreferences.go:UserPreferences`
- [ ] `new_default_user_preferences`/`is_ata_disabled`/`with_overrides`/`module_specifier_preferences`/`parsed_auto_import_file_exclude_patterns`/`is_module_specifier_excluded`/`parse_user_preferences`　`// Go: userpreferences.go:*`
- [ ] **偏离（反射→显式）**：`with_config(map) -> UserPreferences`（unstable raw-name + 嵌套 config-path 两路）、`MarshalJSONTo`/`UnmarshalJSONFrom` 用手写字段表/`serde` 自定义而非 `reflect`（见「偏离」）　`// Go: userpreferences.go:withConfig/MarshalJSONTo/UnmarshalJSONFrom`

### `lsutil/asi.rs`（Go: `internal/ls/lsutil/asi.go`）
- [ ] `PositionIsASICandidate`/`NodeIsASICandidate`/`SyntaxMayBeASICandidate`/`SyntaxRequiresTrailing{CommaOrSemicolonOrASI,FunctionBlock...,ModuleBlock...,Semicolon...}`　`// Go: asi.go:*`

### `lsutil/children.rs`（Go: `internal/ls/lsutil/children.go`）
- [ ] `GetLastChild`/`GetLastToken`/`GetLastVisitedChild`/`GetFirstToken`/`AssertHasRealPosition`（用 scanner 重扫拿 token；`sourceFile.GetOrCreateToken`）　`// Go: children.go:*`

### `lsutil/completednode.rs`（Go: `internal/ls/lsutil/completednode.go`）
- [ ] `PositionBelongsToNode`/`IsCompletedNode`（大 match）/`nodeEndsWith`/`hasChildOfKind`　`// Go: completednode.go:*`

### `lsutil/organizeimports.rs`（Go: `internal/ls/lsutil/organizeimports.go`）
- [ ] 全部比较器/检测器（见前文签名清单 19 个 func）：`GetDetectionLists`/`getOrganizeImports{Ordinal,Unicode}StringComparer`/`CompareModuleSpecifiers`/`CompareImportsOrRequireStatements`/`GetNamedImportSpecifierComparer(WithDetection)`/`GetImport{Specifier,Declaration}Insert{ionIndex,Index}`/`DetectNamedImportOrganizationBySort`/`DetectModuleSpecifierCaseBySort`/`measureSortedness`/`FilterImportDeclarations`/`GetExternalModuleName`　`// Go: organizeimports.go:*`

### `lsutil/symbol_display.rs`（Go: `internal/ls/lsutil/symbol_display.go`）
- [ ] `ScriptElementKind`/`ScriptElementKindModifier`(+`Strings`) + `GetSymbolKind`/`getSymbolKindOfConstructorPropertyMethodAccessorFunctionOrVar`/`GetSymbolModifiers`/`getNormalizedSymbolModifiers`/`getNodeModifiers`/`isDeprecatedDeclaration`/`isLocalVariableOrFunction`　`// Go: symbol_display.go:*`

### `lsutil/utilities.rs`（Go: `internal/ls/lsutil/utilities.go`）
- [ ] `pub fn probably_uses_semicolons(file) -> bool`（被直接测）　`// Go: utilities.go:ProbablyUsesSemicolons`
- [ ] `ShouldUseUriStyleNodeCoreModules`/`GetQuotePreference`/`QuotePreferenceFromString`/`ModuleSpecifierToValidIdentifier`/`ModuleSymbolToValidIdentifier`/`IsNonContextualKeyword`　`// Go: utilities.go:*`

## E3 format（见 `../format/impl.md`，本表略）

## E4 change

### `change/lib.rs`（Go: `internal/ls/change/tracker.go`）
- [ ] `pub struct Tracker` + `NodeOptions`/`trackerEdit`/`deletedNode` + `NewTracker`/`GetChanges`　`// Go: tracker.go:Tracker/NewTracker/GetChanges`
- [ ] 全部公开编辑方法（见前文签名 ~30 个）：`ReplaceNode(WithNodes)`/`ReplaceRange(WithText/WithNodes)`/`InsertText`/`InsertNodeAt(s)`/`InsertNode{After,Before}`/`InsertNodesAfter`/`InsertNodeInListAfter`/`InsertImportSpecifierAtIndex`/`InsertAtTopOfFile`/`InsertMemberAtStart`/`Delete`/`DeleteRange`/`DeleteNode(Range)`/`TryInsertTypeAnnotation`/`ParenthesizeArrowParameters`/`InsertModifierBefore`　`// Go: tracker.go:*`
- [ ] 缩进/位置 helper：`tryComputeIndentationForNewMember`/`getInsertNodeAfterOptions`/`getOptionsForInsertNodeBefore`/`finishDeleteDeclarations`/`findIndentationColumn`　`// Go: tracker.go:*`

### `change/trackerimpl.rs`（Go: `internal/ls/change/trackerimpl.go`）
- [ ] `getTextChangesFromChanges`/`computeNewText`/`getFormattedTextOfNode`（调 `tsgo_format`）/`getNonformattedText`/`getFormatCodeSettingsForWriting`/`GetAdjustedRange`/`getAdjusted{Start,End}Position`/`getEndPositionOfMultilineTrailingComment`/`getInsertionPositionAtSourceFileTop`/`needSemicolonBetween`/`hasCommentsBeforeLineBreak`　`// Go: trackerimpl.go:*`

### `change/delete.rs`（Go: `internal/ls/change/delete.go`）
- [ ] `deleteDeclaration`/`deleteDefaultImport`/`deleteImportBinding`/`deleteVariableDeclaration`/`deleteNode(InList)`/`startPositionToDeleteNodeInList`/`endPositionToDeleteNodeInList`/`positionsAreOnSameLine`/`hasJSDocNodes`　`// Go: delete.go:*`

## E5 autoimport

### `autoimport/index.rs`（Go: `internal/ls/autoimport/index.go`）— **有直接单测**
- [ ] `pub trait Named { fn name(&self) -> &str }` + `pub struct Index<T: Named>{ entries, index: FxHashMap<char, Vec<usize>> }`　`// Go: index.go:Index`
- [ ] `Find(name, case_sensitive)`/`SearchWordPrefix(prefix)`/`insert_as_words(value)`/`Clone(filter)`/`contains_chars_in_order`　`// Go: index.go:Find/SearchWordPrefix/insertAsWords/Clone`

### `autoimport/util.rs`（Go: `internal/ls/autoimport/util.go`）— **有直接单测**
- [ ] `pub fn word_indices(s: &str) -> Vec<usize>`（camelCase/snake/UPPER 分词；被直接测）　`// Go: util.go:wordIndices`
- [ ] `pub fn get_package_realpath_funcs(fs, package_dir) -> (toRealpath, toSymlink)`（符号链接 realpath；被直接测）　`// Go: util.go:getPackageRealpathFuncs`
- [ ] `createCheckerPool`/`getResolvedPackageNames`/`getPackageNamesInNodeModules`/`addPackageJsonDependencies`/`getModuleResolver`/`resolutionHost`/`get(Default)ModuleIDAndFileNameOfModuleSymbol`/`getDefaultLikeExportNameFromDeclaration`　`// Go: util.go:*`

### `autoimport/export.rs`（Go: `internal/ls/autoimport/export.go`）
- [ ] `ModuleID`/`ExportID`/`ExportSyntax`(10)/`Export` + `Name`/`IsRenameable`/`AmbientModuleName`/`IsUnresolvedAlias`/`SymbolToExport`/`tryGetModuleExport`/`extractFirstExport`　`// Go: export.go:*`

### `autoimport/export_stringer_generated.rs`
- [ ] `impl Display for ExportSyntax`（手写或 `strum`，无需复刻 Go stringer 的索引表）　`// Go: export_stringer_generated.go`

### `autoimport/registry.rs`（Go: `internal/ls/autoimport/registry.go`）— **有直接单测**
- [ ] `Registry`/`RegistryBucket`/`BucketState`/`bucketBuildPreferences`/`directory`/`RegistryChange`/`RegistryCloneHost` trait + `BucketStats`/`CacheStats`　`// Go: registry.go:*`
- [ ] `NewRegistry`/`IsPreparedForImportingFile`/`Clone`/`NodeModulesDirectories`/`GetCacheStats`　`// Go: registry.go:*`
- [ ] `registryBuilder` 全链路：`Build`/`updateBucketAndDirectoryExistence`/`markBucketsDirty`/`updateIndexes`/`buildProjectBucket`/`buildNodeModulesBucket`/`updateNodeModulesBucket`/`discoverBucketPackages`/`extractPackage`/`installExtractions`/`computeDependenciesForNodeModulesDirectory`/`getNearestAncestorDirectoryWithPackageJson`/`resolveAmbientModuleName`　`// Go: registry.go:*`（**并发**：rayon 抽取，索引写入同步）

### `autoimport/view.rs`（Go: `internal/ls/autoimport/view.go`）
- [ ] `View` + `NewView`/`Search`/`SearchByExportID`/`GetCompletions`/`getAllowedEndings`/`search`/`FixAndExport`　`// Go: view.go:*`

### `autoimport/fix.rs`（Go: `internal/ls/autoimport/fix.go`）
- [ ] `Fix`/`newImportBinding`/`addToExistingImportFix` + `Edits`/`GetFixes`/`tryAddToExistingImport`/`tryUseExistingNamespaceImport`/`getNewImports`/`getNewRequires`/`makeImport`/`insertImports`/`getImportKind`/`getExistingImports`/`shouldUseRequire`/`computeShouldUseRequire`/`detectSyntax(Indicators)`/`getAddAsTypeOnly`/`promoteFromTypeOnly`/`promoteImportClause`/`CompareFixesFor{Sorting,Ranking}`/`compareModuleSpecifiersFor{Ranking,Sorting}`/`compareModuleSpecifierRelativity`　`// Go: fix.go:*`

### `autoimport/import_adder.rs`（Go: `internal/ls/autoimport/import_adder.go`）
- [ ] `trait ImportAdder` + `importAdder` + `NewImportAdder`/`AddImportFromExportedSymbol`/`AddImportFix`/`Edits`/`HasFixes`/`getNewImportEntry`/`getImportFixForSymbol`/`getAllExportsForSymbol` + 自由函数 `TypeToAutoImportableTypeNode`/`TypeNodeToAutoImportableTypeNode`/`TryGetAutoImportableReferenceFromTypeNode`/`getNameForExportedSymbol`/`importSymbols`/`replaceFirstIdentifierOfEntityName`　`// Go: import_adder.go:*`

### `autoimport/extract.rs`（Go: `internal/ls/autoimport/extract.go`）
- [ ] `symbolExtractor`/`exportExtractor`/`checkerLease`/`extractorStats` + `extractFromFile/Module/ModuleDeclaration/Symbol`/`createExport`/`tryResolveSymbol`/`getModuleID(ForSymbol)`/`getSyntax`/`shouldIgnoreSymbol`/`isUnusableName`/`fileNameForDefaultExportName`　`// Go: extract.go:*`

### `autoimport/aliasresolver.rs`（Go: `internal/ls/autoimport/aliasresolver.go`）
- [ ] `aliasResolver` + `newAliasResolver` + `BindSourceFiles` + ~40 个 host 接口方法（`GetResolvedModule`/`GetSourceFile`/`GetSymlinkCache`/`GetEmitModuleFormatOfFile`/…逐个委托）　`// Go: aliasresolver.go:*`

### `autoimport/specifiers.rs`（Go: `internal/ls/autoimport/specifiers.go`）
- [ ] `View.GetModuleSpecifier(...)`（调 `modulespecifiers`）　`// Go: specifiers.go:GetModuleSpecifier`

## E6 ls 根 · 基础设施（子系统 A）

### `lib.rs`（Go: `languageservice.go`）
- [ ] `pub struct LanguageService` + `NewLanguageService` + 全部访问器/辅助（见子系统 A 清单）　`// Go: languageservice.go:*`

### `host.rs`（Go: `host.go`）
- [ ] `pub trait Host`（11 方法）　`// Go: host.go:Host`

### `api.rs`（Go: `api.go`）
- [ ] `GetSymbolAtPosition`/`GetSymbolAtLocation`/`GetTypeOfSymbol` + 错误常量　`// Go: api.go:*`

### `constants.rs`（Go: `constants.go`）
- [ ] `MODULE_SPECIFIER_RESOLUTION_LIMIT`/`..._CACHE_ATTEMPT_LIMIT`　`// Go: constants.go`

### `diagnostics.rs`（Go: `diagnostics.go`）
- [ ] `ProvideDiagnostics`/`getAllDiagnostics`/`toLSPDiagnostics`　`// Go: diagnostics.go:*`

### `utilities.rs`（Go: `utilities.go`）
- [ ] 范围构造：`createLspRangeFromNode/Bounds/Range`/`createRangeFromNode`/`createLspPosition`　`// Go: utilities.go:createLspRange*`
- [ ] 语义工具：`getMeaningFromLocation/Declaration`/`getIntersectingMeaningFromDeclarations`/`symbolFlagsHaveMeaning`　`// Go: utilities.go:getMeaning*`
- [ ] 定位调整：`getAdjustedLocation(ForDeclaration/Import.../Export...)`/`getContainerNode`/`getContainingNodeIfInHeritageClause`　`// Go: utilities.go:getAdjustedLocation*`
- [ ] 引用/类型参数：`getReferenceAtPosition`/`getPossibleTypeArgumentsInfo`/`getPossibleGenericSignatures`/`getContextualTypeFromParent(OrAncestorTypeNode)`　`// Go: utilities.go:*`
- [ ] 节点谓词族：`isThis`/`isTypeReference`/`isLabelOfLabeledStatement`/`isJumpStatementTarget`/`isRightSideOfPropertyAccess`/`isStaticSymbol`/`isImplementation`/… + `caseClauseTracker`　`// Go: utilities.go:*`
- [ ] `RangeContainsRange`/`startEndContainsRange`/`findContainingList`/`getChildrenFromNonJSDocNode`/`getContainingObjectLiteralElement`/`toContextRange`/`nodeSeenTracker`/`quote`　`// Go: utilities.go:*`

### `displaypartswriter.rs`（Go: `displaypartswriter.go`）
- [ ] `displayPartsWriter`（实现 `EmitTextWriter` trait of printer）+ 全部 `Write*` 方法 + `classificationForSymbol`/`isFirstDeclarationOfSymbolParameter`　`// Go: displaypartswriter.go:*`

### `source_map.rs`（Go: `source_map.go`）
- [ ] `getMappedLocation`/`tryGetSourcePosition(Worker)`/`tryGetGeneratedPosition(Worker)` + `script` 适配器　`// Go: source_map.go:*`

### `crossproject.rs`（Go: `crossproject.go`）
- [ ] `trait Project` + `trait CrossProjectOrchestrator` + `handleCrossProject`（**rayon/scoped 并行 + 保序迭代器**）+ `combineReferences/VsReferences/Implementations/RenameResponse/IncomingCalls`/`combineLocationArray`/`combineResponseLocations`　`// Go: crossproject.go:*`

## E7 ls 根 · 导航类（子系统 F）

- [ ] `definition.rs`：`ProvideDefinition`/`provideDefinitionWorker`/`ProvideTypeDefinition`/`createDefinitionLocations`/`getDeclarationsFromLocation`/`getDeclarationsFromObjectLiteralElement`/`tryGetSignatureDeclaration`/`getSymbolForOverriddenMember`/`getTypeOfSymbolAtLocation`/`getDeclarationsFromType`　`// Go: definition.go:*`
- [ ] `sourcedefinition.rs`：`ProvideSourceDefinition` + `sourceDefResolver`（`resolveFromCheckerInfo`/`resolveTripleSlashReference`/`searchImplementationFile`/`findImplementationFileFromDtsFileName`/`mapDeclarationToSource`/`findDeclarationsInFile`/`getCandidateSourceDeclarationNames`/…）　`// Go: sourcedefinition.go:*`
- [ ] `findallreferences.rs`：`ProvideReferences`/`ProvideVsReferences`/`ProvideImplementations`/`provideImplementationsEx`/`provideSymbolsAndEntries` + 引擎类型 `refOptions`/`refInfo`/`SymbolAndEntries(Data)`/`Definition`/`DefinitionKind`/`ReferenceEntry` + 全部内部搜索逻辑（**最复杂引擎之一**，逐函数对齐）　`// Go: findallreferences.go:*`
- [ ] `documenthighlights.rs`：`ProvideDocumentHighlights`/`ProvideMultiDocumentHighlights`/`provideDocumentHighlightsWorker`/`getSemanticDocumentHighlights`/`getSyntacticDocumentHighlights` + 占用收集族（`getIfElseOccurrences`/`getReturnOccurrences`/`getThrowOccurrences`/`getTryCatchFinallyOccurrences`/`getBreakOrContinue*`/`getModifierOccurrences`/…）　`// Go: documenthighlights.go:*`
- [ ] `callhierarchy.rs`：`ProvidePrepareCallHierarchy`/`ProvideCallHierarchy{Incoming,Outgoing}Calls`/`resolveCallHierarchyDeclaration`/`collectCallSites`/`createCallHierarchyItem`/`getCallHierarchyItemName` + `incomingEntry`/`callSiteCollector`（**跨工程**用 crossproject）　`// Go: callhierarchy.go:*`
- [ ] `importtracker.rs`（Go `importTracker.go`）：`createImportTracker`/`getImportersForExport`/`getSearchesFromDirectImports`/`getImportOrExportSymbol`/`getExportInfo`/`findModuleReferences`/`forEachImport`/`getDirectImportsMap`/…　`// Go: importTracker.go:*`
- [ ] `rename.rs`：`ProvideRename`/`GetRenameInfo`/`symbolAndEntriesToRename`/`getRenameInfoForNode`/`getRenameInfoForModule`/`getTextForRename`/`nodeIsEligibleForRename`/`renameBlockedReason`/`wouldRenameInOtherNodeModules`/`ClientSupports*`　`// Go: rename.go:*`
- [ ] `file_rename.rs`：`GetEditsForFileRename`/`updateImportsForFileRename`/`updateTsconfigFiles`/`getUpdatedImportSpecifier(FromMovedSourceFiles)`/`createPathUpdater`/`updateRelativePath`/`getTsConfigObjectLiteralExpression`　`// Go: file_rename.go:*`

## E8 ls 根 · 信息/呈现类（子系统 G）

- [ ] `hover.rs`：`ProvideHover` + 内部 quickInfo 组装（符号→type+JSDoc+verbosity，用 `displaypartswriter` + `nodebuilder`）　`// Go: hover.go:ProvideHover/...`
- [ ] `completions.rs`：`ProvideCompletion`/`ResolveCompletionItem`/`getCompletionsAtPosition` + 全部补全来源（符号/关键字/JSX/label/auto-import/class member snippet/object literal method）；类型 `ensureItemData` 等。**最大文件，按 Go 分区逐段移植**　`// Go: completions.go:*`
- [ ] `string_completions.rs`：`getStringLiteralCompletions` + `completionsFrom{Types,Properties}`/`pathCompletion`/模块路径补全（用 `module`/`modulespecifiers`/`packagejson`/`tsoptions`）　`// Go: string_completions.go:*`
- [ ] `signaturehelp.rs`：`ProvideSignatureHelp`/`GetSignatureHelpItems` + `callInvocation`/`typeArgsInvocation`/`contextualInvocation`/`invocation`　`// Go: signaturehelp.go:*`
- [ ] `symbols.rs`：`ProvideDocumentSymbols`/`ProvideWorkspaceSymbols`/`getDocumentSymbolsForChildren`/`newDocumentSymbol`/`mergeExpandos`/`getMatchScore`/`compareDeclarationInfos`/`getSymbolKindFromNode`　`// Go: symbols.go:*`
- [ ] `semantictokens.rs`：`ProvideSemanticTokens(Range)`/`SemanticTokensLegend`/`collectSemanticTokens(InRange)`/`classifySymbol`/`tokenFromDeclarationMapping`/`reclassifyByType`/`encodeSemanticTokens`（LSP 5-tuple delta 编码）　`// Go: semantictokens.go:*`
- [ ] `inlay_hints.rs`：`ProvideInlayHint` + `inlayHintState`（visit 族 + `getInlayHintLabelParts`/`typeToInlayHintParts`/`addParameterHints`/`getParameterIdentifierInfoAtPosition`/`shouldShow*`）　`// Go: inlay_hints.go:*`
- [ ] `folding.rs`：`ProvideFoldingRange`/`addNodeOutliningSpans`/`addRegionOutliningSpans`/`visitNode`/`getOutliningSpanForNode`/`spanFor*`/`parseRegionDelimiter`/`createFoldingRange*`　`// Go: folding.go:*`
- [ ] `selectionranges.rs`：`ProvideSelectionRanges`/`getSmartSelectionRange`　`// Go: selectionranges.go:*`
- [ ] `linkedediting.rs`：`ProvideLinkedEditingRange`（+ `jsxTagWordPattern`）　`// Go: linkedediting.go:*`
- [ ] `autoinsert.rs`：`ProvideOnAutoInsert`/`isUnclosedTag`/`isUnclosedFragment`　`// Go: autoinsert.go:*`
- [ ] `codelens.rs`：`ProvideCodeLenses`/`ResolveCodeLens`/`newCodeLensForNode`/`isValid{Implementations,Reference}*Node`　`// Go: codelens.go:*`

## E9 ls 根 · 编辑/重构类（子系统 H）

- [ ] `format.rs`：`ProvideFormatDocument(/Range/OnType)`/`getFormattingEditsFor{Document,Range}`/`getFormattingEditsAfterKeystroke`/`toLSProtoTextEdits`/`getRangeOfEnclosingComment`（转发 `tsgo_format`；**有直接单测**）　`// Go: format.go:*`
- [ ] `organizeimports.rs`：`OrganizeImports`/`organizeImportsWorker`/`coalesce{Imports,Exports}Worker`/`removeUnusedImports`/`groupBy{ModuleSpecifier,NewlineContiguous}`/`getCategorized{Imports,Exports}`/`getTopLevelExportGroups`　`// Go: organizeimports.go:*`
- [ ] `codeactions.rs`：`CodeFixProvider`/`CodeFixContext`/`CodeAction`(+`Compare`)/`CombinedCodeActions`/`codeFixProviders` + `ProvideCodeActions`/`getFixAllQuickFixes`/`createFixAllAction`/`createOrganizeImportsAction`/`convertToLSPCodeAction`　`// Go: codeactions.go:*`
- [ ] `codeactions_importfixes.rs`：`ImportFixProvider` + `getImportCodeActions`/`getAllImportCodeActions`/`getFixInfos`/`getFixesInfoFor{UMD,NonUMD}Import`/`getTypeOnlyPromotionFix`/`addImportFromDiagnostic`/`sortFixInfo`　`// Go: codeactions_importfixes.go:*`
- [ ] `codeactions_fixmissingtypeannotation.rs`：`IsolatedDeclarationsFixProvider` + `isolatedDeclarationsFixer`（`addTypeAnnotation`/`addInlineAssertion`/`extractAsVariable`/`fixIsolatedDeclarationError`/`createNamespaceForExpandoProperties`/…）　`// Go: codeactions_fixmissingtypeannotation.go:*`
- [ ] `codeactions_fixclassincorrectlyimplementsinterface.rs`：`FixClassIncorrectlyImplementsInterfaceProvider` + `getCodeActionsToFix.../getAllCodeActionsToFix...`/`addChanges`/`getMissingMembers`/`getInheritedMembers`/`createImportAdder`　`// Go: codeactions_fixclassincorrectlyimplementsinterface.go:*`
- [ ] `codeactions_missingmemberfixer.rs`：`missingMemberFixer` + `newMissingMemberFixer`/`createMemberFromSymbol`/`createSignatureDeclarationFromSignature(s)`/`createTypeNode`/`createModifiers`/`importTypeNode`/`createIndexSignatureDeclarationFromType`　`// Go: codeactions_missingmemberfixer.go:*`

### Cargo / crate 接线
- [ ] 5 个 `Cargo.toml`：`tsgo_ls_lsconv`/`tsgo_ls_lsutil`/`tsgo_ls_change`/`tsgo_ls_autoimport`/`tsgo_ls`（按「crate 拆分」依赖序声明 path deps）
- [ ] 根 `Cargo.toml` workspace members 追加这 5 个 + `internal/format`
- [ ] 各 crate `lib.rs` 声明子 module + `pub use`（`tsgo_ls` re-export 全部 `Provide*` + `LanguageService` + `Host` + `CrossProjectOrchestrator`）

## TDD 推进顺序（tracer bullet → 增量）

1. **lsconv 先行**（全员地基）：`linemap.rs` → `lib.rs` 的 `LineAndCharacterToPosition`/`PositionToLineAndCharacter`。过 `converters_test.go::TestConvertersInvalidUTF8`（无外部依赖）+ `TestConvertersAgainstJSReference`（需 node，否则 skip）。`FileNameToDocumentURI`/`DocumentUri.FileName` 过 `TestFileNameToDocumentURI`/`TestDocumentURIToFileName`（后者依赖 P8 lsproto，可先 stub 或临时本 crate 实现）。
2. **lsutil**：`formatcodeoptions.rs` + `userpreferences.rs` → 过 `userpreferences_test.go` 4 个 func（roundtrip/serialize/parseUnstable/parseATA）。`utilities.rs::probably_uses_semicolons` → 过 `utilities_test.go`。其余 lsutil（asi/children/completednode/organizeimports/symbol_display）随被依赖逐步补。
3. **autoimport 叶子**：`index.rs`（泛型倒排）→ 过 `index_test.go::TestIndexClone`。`util.rs::word_indices`/`get_package_realpath_funcs` → 过 `util_test.go` 4 个 func（需 `vfstest`）。
4. **format**（见 `../format/`），再 **change**（依赖 format）。
5. **autoimport 主体**：`export`/`extract`/`registry`/`view`/`fix`/`import_adder`/`aliasresolver`/`specifiers` → 过 `registry_test.go` 3 个 func（18 子用例，重型集成，需 `vfstest` + checker）。
6. **ls 根基础设施**（A）：`languageservice`/`host`/`api`/`utilities`/`source_map`/`crossproject`。
7. **format.rs**（ls 根）→ 过 `format_test.go` 3 个 func（onType/range，`LanguageService{}` 零值即可，**不需要 program**）。
8. **各 feature**（F/G/H）：逐个 `Provide*`，细粒度正确性靠 P10 fourslash；本 phase 仅保证编译 + 入口冒烟。

## 与 Go 的已知偏离（divergence）

1. **context.Context → 显式参数 / 取消标志**：ClientCapabilities/locale/UserPreferences/FormatSettings 从 ctx 取的，改成显式传参；取消用 `&Cancel`（PORTING §3）。
2. **`userpreferences.go` 的 `reflect`**：Go 用反射 + struct tag（`raw:`/`config:`）做 unstable/嵌套 config 双路解析 + JSON 序列化。Rust 无运行时反射 → 改为**编译期生成的字段表**（手写 `static FIELDS: &[FieldInfo]` 或 `serde` + 自定义 `Serialize`/`Deserialize`，外加 `proc-macro`/`build.rs` 生成 tag 映射）。**存疑**：是否值得引 proc-macro；建议先手写字段表（字段数固定），过 `userpreferences_test.go` 的 roundtrip/serialize/parse 即达标。
3. **stringer 生成代码**（`export_stringer_generated.go`）→ `#[derive(strum::Display)]` 或手写 `Display`，不复刻索引表机制。
4. **`node.Parent`/symbol/type 句柄** → arena 索引/借用（PORTING §5）。
5. **并行**：`crossproject`/`registry`/多文件搜索从 goroutine+SyncMap → rayon/scoped+dashmap，**结果保序**（按 default-project→all→originalLocation 与文件稳定序）以保证 `combine*` 与断言确定性。
6. **`DocumentUri.FileName()`** 的归属：Go 在 `lsproto`，但 `converters_test.go` 测它 → Rust 归 P8 `tsgo_lsp_lsproto`，本 crate 测试 `// DEFER(phase-8)`。

## 转交 / 推迟（DEFER）

- `// DEFER(phase-8): blocked-by tsgo_lsp_lsproto` — 全部 `lsproto.*Response` 联合类型、`DocumentUri`/`ClientCapabilities`/`GetClientCapabilities` 由 P8 提供；本层只构造。`converters_test.go::TestDocumentURIToFileName` 依赖之。
- `// DEFER(phase-8): blocked-by tsgo_project` — `Host` 的实际实现、`CrossProjectOrchestrator` 的实际实现、`AutoImportRegistry` 的宿主接线在 P8 `project`。
- **细粒度 parity 全面推迟 P10**：补全/悬停/引用/重命名/签名/语义高亮/inlay/折叠/code action/organize imports 等的具体输出正确性，由 `fourslash`（4250 用例）+ `tests/baselines` 端到端对拍兜底（见 `tests.md`「推迟」表）。
