# lsproto: 测试清单（tests.md）

> 本文件记录 `tsgo_lsproto` 本轮（`ResolvedClientCapabilities` 能力树）的测试。
> Go 侧 `lsp_json_test.go`/`lsp_test.go`/`baseproto_test.go` **无任何 resolved 类型 / `Resolve()` 的测试**（`Resolve()` 由 Go 内部生产、极少 (de)serialize）。故按 PORTING §8.6「每个公开类型至少一条行为级测试」自写，expected 取 Go `json:",omitzero"` 标签语义与 LSP spec 字面量。

**完成列图例**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模（resolved 部分）**：0 直接 Go 单测 → 行为由本轮自写 serde 测试 + P10 端到端兜底。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试文件 | 顶层测试函数数 |
|---|---|---|
| （无 resolved 对应 Go 测试） | `internal/lsp/lsproto/resolved_test.rs`（兄弟文件，`use super::*;`，由 `resolved.rs` 末尾 `#[cfg(test)] #[path="resolved_test.rs"] mod tests;` 挂载） | 23 |
| （无 `Resolve()` 对应 Go 测试） | `internal/lsp/lsproto/capabilities_test.rs`（兄弟文件，`use super::*;`，由 `capabilities.rs` 末尾挂载） | 10 |

## resolved 行为级测试（`resolved_test.rs`）

> 这些类型 Go 无 `*_test.go`；下表「依据」列给出 expected 的来源（Go `omitzero` 语义 / LSP spec / `// Go:` 锚点）。
> red→green：仅 tracer（首条）是**真 RED**（普通 derive 序列化出 `{"dynamicRegistration":false}` ≠ `{}`）；其余 serde 行为由 `resolved_object!` 宏统一生成，多为 **green-on-arrival**（诚实标注）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `resolved_did_change_config_default_serializes_empty` | omitzero：全零值 → `{}`（**tracer，真 RED→GREEN**） | `default` → `"{}"` | `lsp_generated.go:ResolvedDidChangeConfigurationClientCapabilities` | ✓ |
| `resolved_did_change_config_set_bool_serializes` | 置位 bool 用 camelCase 键 | `{dynamic_registration:true}` → `{"dynamicRegistration":true}` | 同上 | ✓ |
| `resolved_did_change_config_deserialize_missing_and_unknown` | 缺键→零值、未知键忽略 | `{"unknownFuture":42}` → `dynamic_registration=false` | 同上 | ✓ |
| `nested_all_default_is_omitted` | 嵌套全零结构被父级省略 | `default` → `"{}"` | `lsp_generated.go:ResolvedShowMessageRequestClientCapabilities` | ✓ |
| `nested_non_default_is_emitted` | 非零嵌套被序列化 + round-trip | 置位 → `{"messageActionItem":{"additionalPropertiesSupport":true}}` | 同上 | ✓ |
| `vec_enum_empty_is_omitted` | 空 `Vec<enum>` 省略 | `default` → `"{}"` | `lsp_generated.go:ResolvedClientSymbolKindOptions` | ✓ |
| `vec_enum_non_empty_serializes_as_array` | `Vec<SymbolKind>` → JSON 整数数组 | `[FILE,FUNCTION]` → `{"valueSet":[1,12]}` | 同上 | ✓ |
| `u32_zero_is_omitted_nonzero_emitted` | `u32` 零省略/非零写出 | `range_limit:5000` → 含 `"rangeLimit":5000` | `lsp_generated.go:ResolvedFoldingRangeClientCapabilities` | ✓ |
| `workspace_edit_string_enum_and_vec` | 新字符串枚举（直接字段 + `Vec`）序列化 | `failure_handling=TEXT_ONLY_TRANSACTIONAL` → `"textOnlyTransactional"` | `lsp_generated.go:ResolvedWorkspaceEditClientCapabilities` | ✓ |
| `workspace_edit_default_failure_handling_omitted` | 默认字符串枚举（`""`）被省略 | `default` → `"{}"` | 同上 | ✓ |
| `rename_int_enum_field` | 新整型枚举直接字段 omitzero | `prepare_support_default_behavior=IDENTIFIER` → `1` | `lsp_generated.go:ResolvedRenameClientCapabilities` | ✓ |
| `string_enum_values_and_serde` | 字符串枚举 const 值 + JSON round-trip + 未知值 | spec 字面量 | `lsp_generated.go:ResourceOperationKind/FailureHandlingKind/MarkupKind/CodeActionKind/TokenFormat` | ✓ |
| `int_enum_values_and_serde` | 整型枚举 const 值 + JSON round-trip | spec 字面量 | `lsp_generated.go:SymbolTag/CompletionItemTag/InsertTextMode/CodeActionTag/PrepareSupportDefaultBehavior` | ✓ |
| `empty_object_serializes_to_braces` | `EmptyObject` → `{}`，反序列化忽略成员 | `{"ignored":1}` → ok | `lsp_generated.go:BooleanOrEmptyObject` | ✓ |
| `boolean_or_empty_object_bool_variant` | union bool 变体 | `true` → `boolean=Some(true)` | `lsp_generated.go:BooleanOrEmptyObject` | ✓ |
| `boolean_or_empty_object_object_variant` | union 对象变体 | `{}` → `empty_object=Some` | 同上 | ✓ |
| `boolean_or_empty_object_rejects_string` | 非 bool/对象拒绝 | `"x"` → err | 同上 | ✓ |
| `boolean_or_full_delta_bool_variant` | union bool 变体 | `false` → `boolean=Some(false)` | `lsp_generated.go:BooleanOrClientSemanticTokensRequestFullDelta` | ✓ |
| `boolean_or_full_delta_object_variant` | union 对象变体（`delta`） | `{"delta":true}` → 结构体变体 | 同上 | ✓ |
| `semantic_tokens_request_options_round_trip` | union 字段组合 round-trip | → `{"range":true,"full":{"delta":true}}` | `lsp_generated.go:ResolvedClientSemanticTokensRequestOptions` | ✓ |
| `resolved_client_capabilities_default_empty` | 顶层全零 → `{}` | `default` → `"{}"` | `lsp_generated.go:ResolvedClientCapabilities` | ✓ |
| `resolved_client_capabilities_only_window_group` | 递归 omit：仅置位组被写 | window.work_done_progress=true → `{"window":{"workDoneProgress":true}}` | 同上 | ✓ |
| `resolved_client_capabilities_vs_scalars` | `_vs_*` 标量键名 + i32 + omit | → 含 `"_vs_supportedSnippetVersion":3` | 同上 | ✓ |
| `resolved_client_capabilities_deep_round_trip` | 深层嵌套 round-trip + 值访问 | 多层置位 → 等值 | 同上 | ✓ |
| `resolved_client_capabilities_from_client_json` | 反序列化真实客户端能力子集 | 客户端 JSON → 嵌套字段 | 同上 | ✓ |
| `every_resolved_type_default_serializes_empty` | 全部 78 resolved + `EmptyObject`/`ClientSemanticTokensRequestFullDelta` 的 `default → {}`（§8.6 每类型覆盖） | `T::default()` → `"{}"` | omitzero 不变式 | ✓ |

## 与 impl.md 的对齐核对

- [x] Go 侧 resolved 无 `func Test*` —— 已在本文件说明（0 直接单测，自写行为级测试）
- [x] 每个公开 resolved 类型在 `every_resolved_type_default_serializes_empty` 至少有 1 条覆盖（§8.6）
- [x] expected 取自 Go `omitzero` 语义 / LSP spec 字面量（非随意推断）
- [x] 每条带 `// Go:` 锚（依据列指向 `lsp_generated.go`）
- [x] 与 impl.md 双向对齐（impl.md 每个实现组在此有对应行为测试）

## 续轮：指针版 `ClientCapabilities` 请求树 + `Resolve()`（`capabilities_test.rs`）

> Go 侧对 `Resolve()` 无 `*_test.go`（内部产物、极少 (de)serialize），按 PORTING §8.6 自写行为级测试：反序列化代表性客户端能力 JSON → `resolve()` → 断言 resolved 字段 = Go resolve/default 语义（`derefOr`→present；缺→零值）。
> red→green：每组核心映射均为**真 RED→GREEN**（在接线该组前，对应字段被忽略 → resolved 默认值，断言失败；接线后变绿）。absent→default 与 serde round-trip 类多为 **green-on-arrival**（诚实标注）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `resolve_maps_vs_supported_snippet_version_scalar` | tracer：顶层 `_vs_*` 标量映射（**真 RED→GREEN**：stub `resolve` 返回 default，0≠3） | `{"_vs_supportedSnippetVersion":3}` → `vs_supported_snippet_version==3` | `lsp_generated.go:(*ClientCapabilities).Resolve` | ✓ |
| `resolve_window_present_reflects_capabilities` | Window 组 present→reflected（**真 RED→GREEN**） | `{"window":{"workDoneProgress":true,"showDocument":{"support":true}}}` → 两字段 true | `(*WindowClientCapabilities).resolve` | ✓ |
| `resolve_window_absent_is_default` | Window 缺→默认（green-on-arrival） | `{}` → `window==default` | 同上（nil receiver） | ✓ |
| `resolve_general_present_reflects_capabilities` | General 组 present→reflected（**真 RED→GREEN**），含 markdown 嵌套 + `Vec<PositionEncodingKind>` | `{"general":{"markdown":{"parser":"marked","version":"1.0"},"positionEncodings":["utf-8"]}}` → parser/version/encodings | `(*GeneralClientCapabilities).resolve` | ✓ |
| `resolve_workspace_present_reflects_capabilities` | Workspace 组 present→reflected（**真 RED→GREEN**），含字符串枚举/`Vec<enum>`/深层 symbolKind.valueSet | `{"workspace":{"applyEdit":true,"workspaceEdit":{...},"symbol":{"symbolKind":{"valueSet":[1,12]}},"fileOperations":{"didCreate":true}}}` → applyEdit/failureHandling/resourceOps/symbolKind/didCreate | `(*WorkspaceClientCapabilities).resolve` | ✓ |
| `resolve_text_document_present_reflects_capabilities` | TextDocument 组 present→reflected（**真 RED→GREEN**），含深层 completionItem/int 枚举/`Vec<MarkupKind>`/int-enum/semanticTokens union/`u32` | `{"textDocument":{"completion":{"completionItem":{"snippetSupport":true},"insertTextMode":2},"hover":{"contentFormat":["markdown","plaintext"]},"rename":{"prepareSupportDefaultBehavior":1},"semanticTokens":{"requests":{"full":{"delta":true}}},"foldingRange":{"rangeLimit":5000}}}` → 6 处深层断言 | `(*TextDocumentClientCapabilities).resolve` | ✓ |
| `resolve_absent_groups_are_default` | workspace/general/textDocument 缺→默认（green-on-arrival） | `{}` → 三组 == default | 各组 nil receiver | ✓ |
| `resolve_empty_is_all_default` | 顶层端到端：空 → 全默认 resolved | `{}` → `ResolvedClientCapabilities::default()` | `(*ClientCapabilities).Resolve`（nil/empty） | ✓ |
| `resolve_real_client_capabilities_subset` | 端到端：真实 4 组 + `_vs_` 子集一次解析 | 多组 JSON → 跨 4 组 + vs 标量断言 | `(*ClientCapabilities).Resolve` | ✓ |
| `client_capabilities_serde_round_trip` | 请求树 serde round-trip（公共 API 可用性；green-on-arrival） | 置位树 → `to_string`→`from_str` 等值 + resolve 一致 | 宏 serde（`Option` skip-none） | ✓ |

> 既有 `generated_test.rs` 的 `empty_client_capabilities`（`from_str("{}")`）/`roundtrip_initialize_params_null_process_id` 在 open-object 退役后仍绿（后者字面量改 `ClientCapabilities::default()`）。

## 续轮：服务端 `ServerCapabilities` typed 树（`generated_test.rs`）

> Go 侧 `lsp_json_test.go` 对 `ServerCapabilities` 仅有两条覆盖：`ServerCapabilities empty`（`{}`→ok）与 `InitializeResult capabilities null`（拒 null）——**无任何 populated `ServerCapabilities` 的 serde 测试**（它是服务端产出值，测试少）。故按 PORTING §8.6 自写行为级测试，expected 取 Go `json:",omitzero"` 标签语义 + LSP spec 字面量。本轮测试加在既有 `generated_test.rs`（兄弟文件，`use super::*;`）。
> red→green：**tracer（首条）是真 RED→GREEN**——open-object 退役前 `ServerCapabilities` 是无字段 unit struct，引用 `hover_provider` 字段**编译失败**（观察到 RED）；typed 后变绿。后续每个 provider 组的字段同样是"先引用不存在的字段/类型→编译失败 RED→加类型+字段→GREEN"，逐组推进（诚实：每组首条 RED 真实，组内 round-trip/默认覆盖类为 green-on-arrival）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `server_capabilities_default_serializes_empty` | 全 None → `{}`（omitzero 不变式） | `default` → `"{}"` | `lsp_generated.go:ServerCapabilities` | ✓ |
| `server_capabilities_hover_provider_bool_serializes` | **tracer（真 RED→GREEN）**：typed 树替换 open-object，置位 `hoverProvider` bool 被写出而非丢成 `{}` | `{hover_provider: bool(true)}` → `{"hoverProvider":true}` | 同上 / `BooleanOrHoverOptions` | ✓ |
| `server_capabilities_text_document_sync_kind` | `OrKind` union 的 number 变体序列化为整数 | `kind=INCREMENTAL` → `{"textDocumentSync":2}` | `TextDocumentSyncOptionsOrKind`/`TextDocumentSyncKind` | ✓ |
| `text_document_sync_options_round_trip` | 详细 options（openClose/change/save bool）round-trip | 置位 → `{"openClose":true,"change":1,"save":true}` | `TextDocumentSyncOptions` | ✓ |
| `save_options_object_variant_serde` | `boolean\|SaveOptions` union 对象变体 | `{includeText:true}` → 同字面量 + round-trip | `SaveOptions`/`BooleanOrSaveOptions` | ✓ |
| `server_capabilities_completion_provider_round_trip` | 直接 `CompletionOptions`（triggerCharacters/resolveProvider/嵌套 completionItem）round-trip | 置位 → 嵌套 JSON | `CompletionOptions`/`ServerCompletionItemOptions` | ✓ |
| `server_capabilities_signature_help_provider` | 直接 `SignatureHelpOptions`（triggerCharacters）round-trip | 置位 → `{"signatureHelpProvider":{"triggerCharacters":["(",","]}}` | `SignatureHelpOptions` | ✓ |
| `server_capabilities_definition_provider_bool` | `BooleanOrDefinitionOptions` bool 变体 | `bool(true)` → `{"definitionProvider":true}` | `BooleanOrDefinitionOptions` | ✓ |
| `server_capabilities_references_provider_options` | `BooleanOrReferenceOptions` 对象变体 round-trip | 置位 → `{"referencesProvider":{"workDoneProgress":true}}` | `ReferenceOptions`/`BooleanOrReferenceOptions` | ✓ |
| `server_capabilities_document_symbol_provider_options` | `BooleanOrDocumentSymbolOptions` 对象变体（label）round-trip | 置位 → `{"documentSymbolProvider":{"label":"TS"}}` | `DocumentSymbolOptions` | ✓ |
| `server_capabilities_code_action_provider_options` | `Vec<CodeActionKind>` + resolveProvider round-trip | 置位 → `{"codeActionProvider":{"codeActionKinds":["quickfix","refactor"],"resolveProvider":true}}` | `CodeActionOptions`（复用 resolved `CodeActionKind`） | ✓ |
| `server_capabilities_document_formatting_provider_bool` | `BooleanOrDocumentFormattingOptions` bool 变体 | `bool(true)` → `{"documentFormattingProvider":true}` | `BooleanOrDocumentFormattingOptions` | ✓ |
| `server_capabilities_rename_provider_options` | `BooleanOrRenameOptions` 对象变体（prepareProvider）round-trip | 置位 → `{"renameProvider":{"prepareProvider":true}}` | `RenameOptions` | ✓ |
| `server_capabilities_workspace_symbol_provider_options` | `BooleanOrWorkspaceSymbolOptions` 对象变体（resolveProvider）round-trip | 置位 → `{"workspaceSymbolProvider":{"resolveProvider":true}}` | `WorkspaceSymbolOptions` | ✓ |
| `server_capabilities_semantic_tokens_provider_options` | options 变体：必填 legend + range(bool) + full(delta) round-trip | 置位 → `{"semanticTokensProvider":{"legend":{"tokenTypes":["namespace"],"tokenModifiers":[]},"range":true,"full":{"delta":true}}}` | `SemanticTokensOptions`/`Legend`/`FullDelta` | ✓ |
| `semantic_tokens_legend_requires_token_types` | 必填字段缺失 → Go `errMissing` 文案 | `{"tokenModifiers":[]}` → `missing required properties: tokenTypes` | `SemanticTokensLegend`（`reqnn`） | ✓ |
| `semantic_tokens_provider_registration_variant` | `OrRegistrationOptions` 按 `documentSelector` 键判别 → registration 变体（raw JSON） | `{"documentSelector":[...],...}` → `registration_options=Some` | `SemanticTokensOptionsOrRegistrationOptions` | ✓ |
| `server_capabilities_position_encoding` | `positionEncoding` 字符串枚举 | `UTF16` → `{"positionEncoding":"utf-16"}` | `PositionEncodingKind` | ✓ |
| `server_capabilities_deferred_and_bool_fields_round_trip` | 深 provider 建 raw JSON + `*bool` provider round-trip + Go 字段序 | `{executeCommandProvider, customSourceDefinitionProvider, _vs_referencesProvider}` → 同序字面量 | `ServerCapabilities`（DEFER 字段） | ✓ |
| `server_capabilities_field_order` | 多字段序列化按 Go 声明序（positionEncoding 在前） | 4 字段 → 有序字面量 | `lsp_generated.go:ServerCapabilities`（字段序） | ✓ |
| `every_simple_server_option_default_serializes_empty` | §8.6 每类型覆盖：全可选 option struct `default → {}` | `T::default()` → `"{}"`（14 类型） | omitzero 不变式 | ✓ |

> 既有 `empty_server_capabilities`（`from_str("{}")`）与 `InitializeResult capabilities null`（拒 null）在 open-object 退役后仍绿（typed struct 全字段可选、`InitializeResult.capabilities` 仍 `reqnn`）。

## 续轮：服务端 provider 注册选项树（registration-options）（`generated_test.rs`）

> 本轮把 `ServerCapabilities` 的 22 个 raw-JSON DEFER provider 中的 **21 个**落成 typed option/registration 树。Go 侧 `lsp_json_test.go` 对这些 provider **无 populated serde 测试**（服务端产出值），按 PORTING §8.6 自写行为级 serde 测试，expected 取 Go `json` 标签语义 + LSP spec 字面量，测试加在既有 `generated_test.rs`。
> red→green：每个 provider 字段从 `serde_json::Value` 换成 typed `Option<T>`，对应测试**先引用不存在的 typed 类型/字段 → 编译失败 RED → 加类型 + 换字段 → GREEN**（与上一轮 ServerCapabilities 同一 RED 约定）。`ExecuteCommandOptions`（tracer）/`DocumentOnTypeFormattingOptions`/各 boolean-or-options/`DiagnosticOptions`/新 `boolean_or_options_or_registration!` 宏的首个用例（declaration）均为**真 RED→GREEN**；同宏后续 11 个 triple-union 为 **green-on-arrival**（诚实标注）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `server_capabilities_execute_command_provider_options` | **tracer（真 RED→GREEN）**：typed `ExecuteCommandOptions` 替换 raw JSON（`reqnn commands`） | 置位 → `{"executeCommandProvider":{"commands":["foo.bar","foo.baz"]}}` | `lsp_generated.go:ExecuteCommandOptions` | ✓ |
| `server_capabilities_document_on_type_formatting_provider_options` | `req firstTriggerCharacter` + `opt moreTriggerCharacter` round-trip | 置位 → `{"documentOnTypeFormattingProvider":{"firstTriggerCharacter":"{","moreTriggerCharacter":[";","\n"]}}` | `lsp_generated.go:DocumentOnTypeFormattingOptions` | ✓ |
| `document_on_type_formatting_requires_first_trigger_character` | 必填缺失 → Go `errMissing` 文案 | `{"moreTriggerCharacter":[";"]}` → `missing required properties: firstTriggerCharacter` | 同上（missingFirstTriggerCharacter） | ✓ |
| `server_capabilities_code_lens_provider_options` | `CodeLensOptions`（resolveProvider）round-trip | 置位 → `{"codeLensProvider":{"resolveProvider":true}}` | `lsp_generated.go:CodeLensOptions` | ✓ |
| `server_capabilities_document_link_provider_options` | `DocumentLinkOptions`（workDoneProgress+resolveProvider）round-trip | 置位 → `{"documentLinkProvider":{"workDoneProgress":true,"resolveProvider":false}}` | `lsp_generated.go:DocumentLinkOptions` | ✓ |
| `server_capabilities_vs_on_auto_insert_provider_options` | `VsOnAutoInsertOptions`（`reqnn _vs_triggerCharacters`）round-trip | 置位 → `{"_vs_onAutoInsertProvider":{"_vs_triggerCharacters":[">","/"]}}` | `lsp_generated.go:VsOnAutoInsertOptions` | ✓ |
| `server_capabilities_document_highlight_provider_bool` | `BooleanOrDocumentHighlightOptions` bool 变体 | `bool(true)` → `{"documentHighlightProvider":true}` | `lsp_generated.go:BooleanOrDocumentHighlightOptions` | ✓ |
| `server_capabilities_document_range_formatting_provider_options` | options 变体（rangesSupport）round-trip | 置位 → `{"documentRangeFormattingProvider":{"rangesSupport":true}}` | `lsp_generated.go:DocumentRangeFormattingOptions` | ✓ |
| `server_capabilities_inline_completion_provider_options` | `BooleanOrInlineCompletionOptions` options 变体 round-trip | 置位 → `{"inlineCompletionProvider":{"workDoneProgress":true}}` | `lsp_generated.go:BooleanOrInlineCompletionOptions` | ✓ |
| `server_capabilities_diagnostic_provider_options` | `DiagnosticOptions`：`req` bool 始终序列化 | 置位 → `{"diagnosticProvider":{"identifier":"ts","interFileDependencies":true,"workspaceDiagnostics":false}}` | `lsp_generated.go:DiagnosticOptions` | ✓ |
| `diagnostic_provider_registration_variant` | 按 `documentSelector` 键 → registration 变体（raw JSON） | `{"documentSelector":[...],...}` → `registration_options=Some` | `lsp_generated.go:DiagnosticOptionsOrRegistrationOptions.UnmarshalJSONFrom` | ✓ |
| `server_capabilities_declaration_provider_bool` | **新宏首用（真 RED→GREEN）**：triple-union bool 变体 | `bool(true)` → `{"declarationProvider":true}` | `lsp_generated.go:BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions` | ✓ |
| `server_capabilities_declaration_provider_options` | triple-union typed options 变体 round-trip | 置位 → `{"declarationProvider":{"workDoneProgress":true}}` | `lsp_generated.go:DeclarationOptions` | ✓ |
| `declaration_provider_registration_variant` | triple-union 按 `documentSelector` → registration（raw JSON） | `{"documentSelector":[...],"id":"reg1"}` → `registration_options=Some` | 同上（UnmarshalJSONFrom） | ✓ |
| `server_capabilities_inlay_hint_provider_options` | inlayHint options 变体（含额外 `resolveProvider`） | 置位 → `{"inlayHintProvider":{"resolveProvider":true}}` | `lsp_generated.go:InlayHintOptions` | ✓ |
| `server_capabilities_color_provider_bool` | color triple-union bool 变体（共享宏，green-on-arrival） | `bool(true)` → `{"colorProvider":true}` | `lsp_generated.go:BooleanOrDocumentColorOptionsOrDocumentColorRegistrationOptions` | ✓ |
| `type_definition_provider_registration_variant` | typeDefinition triple-union registration 派发（green-on-arrival） | `{"documentSelector":[...]}` → `registration_options=Some` | `lsp_generated.go:BooleanOrTypeDefinitionOptionsOrTypeDefinitionRegistrationOptions` | ✓ |
| `server_capabilities_all_triple_union_providers_round_trip` | 其余 8 个 triple-union（impl/folding/selection/callHierarchy/linkedEditing/moniker/typeHierarchy/inlineValue）深层 round-trip（green-on-arrival） | 多组置位 → `to_string`→`from_str` 等值 | `lsp_generated.go:ServerCapabilities`（provider 组） | ✓ |
| `every_simple_server_option_default_serializes_empty`（扩展） | §8.6：全 `opt` option struct `default → {}`（新增 17 个：CodeLens/DocumentLink/DocumentHighlight/DocumentRangeFormatting/InlineCompletion/Declaration/TypeDefinition/Implementation/DocumentColor/FoldingRange/SelectionRange/CallHierarchy/LinkedEditingRange/Moniker/TypeHierarchy/InlineValue/InlayHint） | `T::default()` → `"{}"` | omitzero 不变式 | ✓ |

> 既有 `server_capabilities_deferred_and_bool_fields_round_trip` 的 `execute_command_provider` 字面量本轮从 raw `json!({...})` 改为 typed `ExecuteCommandOptions{...}`（同一期望 JSON，仍绿）。`req`/`reqnn` 字段的 option struct（`ExecuteCommandOptions`/`DocumentOnTypeFormattingOptions`/`VsOnAutoInsertOptions`/`DiagnosticOptions`）因 `default ≠ {}` 不纳入 `every_simple_server_option_default_serializes_empty`；`BooleanOr*` union `default` 序列化为 err（恰一变体置位），同样不纳入。

## 续轮：registration-options base tree（`generated_test.rs`）

> 本轮移植 registration-options 基底类型树并把全部 14 个 raw-JSON registration 槽升级为 typed `*RegistrationOptions`。Go 侧 `lsp_json_test.go` 对这些 registration 类型**无 populated serde 测试**（服务端产出值），按 PORTING §8.6 自写行为级 serde 测试，expected 取 Go `json` 标签语义 + LSP spec 字面量，测试加在既有 `generated_test.rs`。
> red→green：每个基底类型与两个手写 union 的升级、以及 triple-union 宏升级的首例（declaration）均为**真 RED→GREEN**（先引用不存在的 typed 类型/字段 → 编译失败 RED → 加类型/改宏 → GREEN）；同宏后续 11 个 triple-union registration 为 **green-on-arrival**（诚实标注），由综合 round-trip + 每类型 default 覆盖。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `static_registration_options_round_trips` | **tracer（真 RED→GREEN）**：`id` 置位 round-trip | `{id:"reg1"}` → `{"id":"reg1"}` | `lsp_generated.go:StaticRegistrationOptions` | ✓ |
| `static_registration_options_default_is_empty` | omitzero：`id` 缺→`{}`、`{}`→`id=None` | `default` → `"{}"` | 同上 | ✓ |
| `pattern_or_relative_pattern_string_variant` | string 变体 typed round-trip | `"**/*.ts"` → `pattern=Some` | `lsp_generated.go:PatternOrRelativePattern` | ✓ |
| `pattern_or_relative_pattern_relative_variant_raw` | 对象→ raw `RelativePattern`（DEFER）round-trip | `{baseUri,pattern}` → `relative_pattern=Some` | 同上（对象臂） | ✓ |
| `text_document_filter_language_round_trips` | `language` 必填变体 round-trip（scheme opt） | `{language,scheme}` → 同字面量 | `lsp_generated.go:TextDocumentFilterLanguage` | ✓ |
| `text_document_filter_language_requires_language` | `language` 缺→`errMissing` | `{scheme:"file"}` → `missing required properties: language` | 同上 | ✓ |
| `text_document_filter_scheme_and_pattern_variants` | scheme/pattern 必填变体 round-trip | 置位 → `{"scheme":...}` / `{"pattern":...}` | `TextDocumentFilterScheme/Pattern` | ✓ |
| `document_filter_union_dispatch` | union 按判别字段依序派发（language→scheme→pattern） | 三对象 → 对应变体 | `TextDocumentFilterLanguageOrSchemeOrPattern.UnmarshalJSONFrom` | ✓ |
| `document_selector_or_null_array_variant` | 数组变体 round-trip（多 filter） | `[{language},{scheme}]` → `Some(vec 2)` | `lsp_generated.go:DocumentSelectorOrNull` (array) | ✓ |
| `document_selector_or_null_null_variant` | `null` → `None`；`default → null` | `null` → `None`；`default` → `"null"` | `DocumentSelectorOrNull.MarshalJSONTo` (nil→null) | ✓ |
| `text_document_registration_options_round_trips` | `req documentSelector` round-trip | `{documentSelector:[{language}]}` → 同字面量 | `lsp_generated.go:TextDocumentRegistrationOptions` | ✓ |
| `text_document_registration_options_null_selector_and_missing` | `null` 选择器 + 缺键→`errMissing` + `default→{"documentSelector":null}` | `{}` → `missing required properties: documentSelector` | 同上 | ✓ |
| `declaration_provider_registration_variant_typed` | **tracer（真 RED→GREEN）**：triple-union 宏 registration 臂 typed（`id`/`documentSelector`/`workDoneProgress` 可访问）+ byte-for-byte round-trip（Go 字段序 workDoneProgress,documentSelector,id） | 注册 JSON → typed 字段 + 同字面量 | `lsp_generated.go:DeclarationRegistrationOptions` | ✓ |
| `diagnostic_provider_registration_variant_typed` | **真 RED→GREEN**：手写 union → typed `DiagnosticRegistrationOptions`（`req` 非指针 bool）+ byte-for-byte round-trip | 注册 JSON → typed 字段 + 同字面量 | `lsp_generated.go:DiagnosticRegistrationOptions` | ✓ |
| `semantic_tokens_provider_registration_variant_typed` | **真 RED→GREEN**：手写 union → typed `SemanticTokensRegistrationOptions`（`reqnn legend`）+ byte-for-byte round-trip | 注册 JSON → `legend.token_types`/`id` + 同字面量 | `lsp_generated.go:SemanticTokensRegistrationOptions` | ✓ |
| `all_triple_union_registration_variants_round_trip` | 12 个 triple-union registration 变体全置位 deep round-trip（green-on-arrival 覆盖） | 12 provider → `to_string`→`from_str` 等值 | `lsp_generated.go:ServerCapabilities`（triple-union 组） | ✓ |
| `inlay_hint_registration_variant_field_order` | inlayHint registration 含 `resolveProvider` + Go 字段序 + `documentSelector:null` 仍派发 registration | `{workDoneProgress,resolveProvider,documentSelector:null,id}` → 同字面量 | `lsp_generated.go:InlayHintRegistrationOptions` | ✓ |
| `moniker_registration_variant_has_no_id` | moniker registration **无 `id` 字段**（Go quirk）：输入 `id` 被忽略、不回写 | `{...,id:"ignored"}` → 重序列化丢弃 `id` | `lsp_generated.go:MonikerRegistrationOptions` (no Id) | ✓ |
| `every_registration_options_default_serializes_document_selector` | §8.6 每类型覆盖：13 个 registration `default → {"documentSelector":null}` + Diagnostic/SemanticTokens 各含其 `req`/`reqnn` 字段 | `T::default()` → 含 `documentSelector` 键 | 必填 `documentSelector` 不变式 | ✓ |

> 既有 `declaration_provider_registration_variant` / `type_definition_provider_registration_variant` / `diagnostic_provider_registration_variant` / `semantic_tokens_provider_registration_variant`（断言 `.registration_options.is_some()`）在字段类型从 `Option<serde_json::Value>` 升级为 typed `Option<*RegistrationOptions>` 后**仍绿**（`.is_some()` 与构造 `registration_options: None` 对任意 `Option<T>` 不变）；公共 API 仅字段**内层类型**收紧，既有测试字面量无需改动。

## 续轮：`WorkspaceOptions` 子树 + `RelativePattern` 对象变体（`generated_test.rs`）

> 本轮把 `ServerCapabilities` 最后一个 raw-JSON DEFER 槽（`workspace`）与 `PatternOrRelativePattern` 的 `RelativePattern` 对象变体升级为 typed 树。Go 侧 `lsp_json_test.go` 对这些 workspace/relative-pattern 类型**无 populated serde 测试**（服务端产出值），按 PORTING §8.6 自写行为级 serde 测试，expected 取 Go `json` 标签语义 + LSP spec 字面量，测试加在既有 `generated_test.rs`。
> red→green：每个新类型的**首个引用测试**与两处 raw→typed **swap/upgrade** 均为**真 RED→GREEN**（先引用不存在的 typed 类型/字段 → 编译失败 RED → 加类型/换字段 → GREEN）；同类型的另一 union 臂 / round-trip / `default→{}` / errMissing 文案多为 **green-on-arrival**（诚实标注）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `workspace_folders_server_capabilities_supported_round_trip` | **tracer（真 RED→GREEN）**：`supported` 置位 round-trip | `{supported:true}` → `{"supported":true}` | `lsp_generated.go:WorkspaceFoldersServerCapabilities` | ✓ |
| `workspace_folders_change_notifications_string_variant` | `changeNotifications` 字符串臂（registration id） | `"workspace/..."` → `string=Some` | 同上 / `StringOrBoolean` | ✓ |
| `workspace_folders_change_notifications_bool_variant` | `changeNotifications` 布尔臂 | `true` → `boolean=Some(true)` | 同上 | ✓ |
| `string_or_boolean_union_dispatch` | union 派发：string/bool 接受、其余拒绝 | `"x"`/`false`/`42` → string/bool/err | `lsp_generated.go:StringOrBoolean.UnmarshalJSONFrom` | ✓ |
| `workspace_folders_server_capabilities_default_empty` | §8.6：全可选 → `{}` | `default` → `"{}"` | omitzero 不变式 | ✓ |
| `file_operation_pattern_round_trip` | **tracer（真 RED→GREEN）**：`glob`/`matches`(string enum)/`options` round-trip | 置位 → `{"glob":...,"matches":"file","options":{"ignoreCase":true}}` | `lsp_generated.go:FileOperationPattern` | ✓ |
| `file_operation_filter_round_trip` | **真 RED→GREEN**：`scheme?` + `pattern`(reqnn) round-trip | 置位 → `{"scheme":"file","pattern":{...}}` | `lsp_generated.go:FileOperationFilter` | ✓ |
| `file_operation_registration_options_round_trip` | **真 RED→GREEN**：`filters`(reqnn) round-trip | 置位 → `{"filters":[{...}]}` | `lsp_generated.go:FileOperationRegistrationOptions` | ✓ |
| `file_operation_registration_options_requires_filters` | `filters` 缺失 → `errMissing` | `{}` → `missing required properties: filters` | 同上 | ✓ |
| `file_operation_options_round_trip` | **真 RED→GREEN**：6 槽（置位 didCreate/willRename）Go 字段序 | 置位 → 同序字面量 | `lsp_generated.go:FileOperationOptions` | ✓ |
| `file_operation_options_default_empty` | §8.6：全可选 → `{}` | `default` → `"{}"` | omitzero 不变式 | ✓ |
| `file_operation_pattern_requires_glob` | `glob` 缺失 → `errMissing`（green-on-arrival） | `{"matches":"file"}` → `missing required properties: glob` | `lsp_generated.go:FileOperationPattern` | ✓ |
| `file_operation_filter_pattern_required_and_rejects_null` | `pattern` 必填 + 拒 null（green-on-arrival） | `{"scheme":...}`→missing；`{"pattern":null}`→err | `lsp_generated.go:FileOperationFilter` | ✓ |
| `text_document_content_options_round_trip` | **tracer（真 RED→GREEN）**：`schemes`(reqnn) round-trip + 缺失 errMissing | 置位 → `{"schemes":[...]}` | `lsp_generated.go:TextDocumentContentOptions` | ✓ |
| `text_document_content_registration_options_round_trip` | `schemes` + `id?` Go 字段序 round-trip | 置位 → `{"schemes":[...],"id":"reg-1"}` | `lsp_generated.go:TextDocumentContentRegistrationOptions` | ✓ |
| `text_document_content_union_prefers_options` | try-order：options 总先匹配（含多余 `id`），registration 显式构造仍序列化（green-on-arrival） | `{schemes,...}` → options 臂 | `lsp_generated.go:TextDocumentContentOptionsOrRegistrationOptions.UnmarshalJSONFrom` | ✓ |
| `workspace_options_round_trip` | **真 RED→GREEN**：3 成员组装 Go 字段序 round-trip | 置位 → 同序字面量 | `lsp_generated.go:WorkspaceOptions` | ✓ |
| `server_capabilities_workspace_typed` | **headline（真 RED→GREEN）**：`workspace` 从 raw `Value` 换 typed `WorkspaceOptions`，深层字段可访问 + byte-for-byte round-trip | `{"workspace":{...}}` → typed 字段 + 同字面量 | `lsp_generated.go:ServerCapabilities`(workspace) | ✓ |
| `workspace_options_default_empty` | §8.6：全可选 → `{}` | `default` → `"{}"` | omitzero 不变式 | ✓ |
| `workspace_folder_round_trip` | **tracer（真 RED→GREEN）**：`uri`/`name`(req) round-trip + 缺失 errMissing | 置位 → `{"uri":...,"name":...}` | `lsp_generated.go:WorkspaceFolder` | ✓ |
| `workspace_folder_or_uri_dispatch` | union 派发：`{`→folder、`"`→URI、数字拒绝 | 三 input → 对应臂/err | `lsp_generated.go:WorkspaceFolderOrURI.UnmarshalJSONFrom` | ✓ |
| `relative_pattern_round_trip_both_base_uri_arms` | `RelativePattern` 两种 `baseUri` 臂（URI string / WorkspaceFolder 对象）round-trip | 两 input → 同字面量 | `lsp_generated.go:RelativePattern` | ✓ |
| `pattern_or_relative_pattern_relative_variant_typed` | **headline（真 RED→GREEN）**：`relative_pattern` 从 raw `Value` 换 typed `RelativePattern`，`base_uri`/`pattern` 可访问 + byte-for-byte round-trip | `{"baseUri":...,"pattern":...}` → typed 字段 + 同字面量 | `lsp_generated.go:PatternOrRelativePattern.UnmarshalJSONFrom`(object) | ✓ |

> 既有 `pattern_or_relative_pattern_relative_variant_raw`（断言 `.relative_pattern.is_some()` + round-trip）在 `relative_pattern` 字段类型从 `Option<serde_json::Value>` 升级为 typed `Option<RelativePattern>` 后**仍绿**（`.is_some()` 与 byte-for-byte round-trip 不变）；公共 API 仅字段**内层类型**收紧，既有测试字面量无需改动。`every_simple_server_option_default_serializes_empty` 扩展加入 `WorkspaceOptions`/`WorkspaceFoldersServerCapabilities`/`FileOperationOptions`/`FileOperationPatternOptions`（全可选 → `{}`）。

## 续轮：请求/结果参数 + 通知类型 concrete-typed raw-JSON 收紧（`generated_test.rs`）

> 本轮把请求/结果参数 + 通知类型里 17 处 concrete-typed `serde_json::Value` 收紧为 typed `Option<T>`/`req T`。Go 侧 `lsp_json_test.go`/`lsp_test.go` 对这些类型只有零散覆盖（多为服务端/客户端产出值），按 PORTING §8.6 自写行为级 serde 测试，expected 取 Go `json` 标签语义 + LSP spec 字面量，测试加在既有 `generated_test.rs`。
> red→green：每个新类型的**首个引用/swap 测试**为**真 RED→GREEN**（字段在 raw-`Value` 态无 typed 子字段 → 编译失败 RED → 加类型 + 换字段 → GREEN）；同类型的另一 union 臂 / round-trip / `default→{}` / required-missing / null 守卫多为 **green-on-arrival**（诚实标注）。本轮 +30 单测（260 → 290），doctest 不变（15）。

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `hover_contents_markup_content_variant` | **tracer（真 RED→GREEN）**：`Hover.contents` typed union，`kind` 对象臂 → `MarkupContent` + byte-for-byte round-trip | `{"contents":{"kind":"markdown","value":"# hi"}}` → `markup_content` | `lsp_generated.go:Hover / MarkupContent` | ✓ |
| `hover_contents_string_variant` | string 臂（green-on-arrival） | `{"contents":"plain hover text"}` → `string=Some` | `...MarkedStrings`(string case) | ✓ |
| `hover_contents_marked_string_with_language_variant` | `language` 对象臂（green-on-arrival） | `{"contents":{"language":"typescript","value":...}}` → `marked_string_with_language` | 同上(language) | ✓ |
| `hover_contents_marked_strings_array_variant` | `MarkedString[]` 臂（元素 union；green-on-arrival） | `{"contents":["text one",{"language":"ts","value":"x"}]}` → vec 2 | 同上(array) / `StringOrMarkedStringWithLanguage` | ✓ |
| `markup_content_requires_value` | `MarkupContent` 缺 `value` → `errMissing`（green-on-arrival） | `{"kind":"markdown"}` → `missing required properties: value` | `lsp_generated.go:MarkupContent` | ✓ |
| `completion_item_documentation_markup_content_variant` | **tracer（真 RED→GREEN）**：`CompletionItem.documentation` typed `StringOrMarkupContent` 对象臂 + round-trip | `{"label":"x","documentation":{"kind":"plaintext","value":"docs"}}` → `markup_content` | `lsp_generated.go:CompletionItem / StringOrMarkupContent` | ✓ |
| `completion_item_documentation_string_variant` | string 臂（green-on-arrival） | `{"label":"x","documentation":"plain docs"}` → `string=Some` | `StringOrMarkupContent`(string) | ✓ |
| `inlay_hint_tooltip_markup_variant` | `InlayHint.tooltip` 共享 union 对象臂（green-on-arrival） | tooltip markdown 对象 → `markup_content.value` | `lsp_generated.go:InlayHint` | ✓ |
| `inlay_hint_label_part_tooltip_string_variant` | `InlayHintLabelPart.tooltip` string 臂（green-on-arrival） | `{"value":"lp","tooltip":"hello"}` → `string=Some` | `lsp_generated.go:InlayHintLabelPart` | ✓ |
| `initialize_params_client_info_and_trace_typed` | **tracer（真 RED→GREEN）**：`clientInfo` typed `ClientInfo` + `trace` typed `TraceValue` + round-trip | `{...,"clientInfo":{"name":"vscode","version":"1.9"},...,"trace":"verbose"}` → name/version/`TraceValue::VERBOSE` | `lsp_generated.go:InitializeParams` | ✓ |
| `trace_value_const_values_and_serde` | `TraceValue` const 值 + JSON round-trip + 未知值（green-on-arrival） | spec 字面量 off/messages/verbose | `lsp_generated.go:TraceValue` | ✓ |
| `client_info_version_null_rejected_and_name_required` | `version` 拒 null + 缺 `name`→`errMissing`（green-on-arrival） | `{"name":"c","version":null}`→errNull；`{"version":"1"}`→missing name | `lsp_generated.go:ClientInfo` | ✓ |
| `initialize_result_server_info_typed` | `InitializeResult.serverInfo` typed `ServerInfo` round-trip + 默认省略（green-on-arrival） | `{"capabilities":{},"serverInfo":{"name":"tsgo","version":"0.1"}}` ↔ 同字面量 | `lsp_generated.go:InitializeResult/ServerInfo` | ✓ |
| `initialize_params_trace_null_rejected` | `trace` 拒 null（errNull guard；green-on-arrival） | `{...,"trace":null}` → errNull | `lsp_generated.go:InitializeParams`(trace) | ✓ |
| `initialize_params_root_path_and_workspace_folders_typed` | **tracer（真 RED→GREEN）**：`rootPath` typed `StringOrNull` + `workspaceFolders` typed `WorkspaceFoldersOrNull` + round-trip | `{...,"rootPath":"/ws",...,"workspaceFolders":[{"uri":...,"name":"a"}]}` → string/folders | `lsp_generated.go:InitializeParams` | ✓ |
| `string_or_null_dispatch` | `StringOrNull` null→None / string / 拒其他 kind（green-on-arrival） | `null`/`"abc"`/`42` → None/Some/err | `lsp_generated.go:StringOrNull` | ✓ |
| `workspace_folders_or_null_dispatch` | `WorkspaceFoldersOrNull` null→None / array / 拒其他 kind（green-on-arrival） | `null`/`[{...}]`/`42` → None/Some/err | `lsp_generated.go:WorkspaceFoldersOrNull` | ✓ |
| `completion_item_label_details_tags_insert_text_mode_typed` | **tracer（真 RED→GREEN）**：`labelDetails`/`tags`(`Vec<CompletionItemTag>`)/`insertTextMode`(`InsertTextMode`) typed + round-trip | `{...,"labelDetails":{...},"tags":[1],"insertTextMode":2}` → 三字段 | `lsp_generated.go:CompletionItem` | ✓ |
| `completion_item_label_details_default_and_null` | `CompletionItemLabelDetails` `default→{}` + `detail` 拒 null（green-on-arrival） | `default`→`{}`；`{"detail":null}`→errNull | `lsp_generated.go:CompletionItemLabelDetails` | ✓ |
| `completion_item_tags_null_rejected` | `tags` 拒 null（errNull guard；green-on-arrival） | `{"label":"x","tags":null}` → errNull | `lsp_generated.go:CompletionItem`(tags) | ✓ |
| `completion_item_command_typed` | **tracer（真 RED→GREEN）**：`CompletionItem.command` typed `Command`，`arguments` 元素保留 `LSPAny` + Go 字段序 round-trip | `{...,"command":{"title":"Save","command":"ts.save","arguments":[1,"a"]}}` → title/command/arguments | `lsp_generated.go:CompletionItem/Command` | ✓ |
| `command_requires_title_and_command` | `Command` 缺 `command`→`errMissing`（green-on-arrival） | `{"title":"t"}` → `missing required properties: command` | `lsp_generated.go:Command` | ✓ |
| `inlay_hint_label_part_command_typed` | `InlayHintLabelPart.command` 共享 `Command` round-trip（green-on-arrival） | `{"value":"lp","command":{"title":"Go","command":"ts.go"}}` ↔ 同字面量 | `lsp_generated.go:InlayHintLabelPart` | ✓ |
| `call_hierarchy_incoming_call_from_typed` | **tracer（真 RED→GREEN）**：`CallHierarchyIncomingCall.from` typed `CallHierarchyItem` + Go 字段序 round-trip（`data` carrier 保留 raw） | 注册 JSON → name/kind(`FUNCTION`)/uri/selectionRange | `lsp_generated.go:CallHierarchyIncomingCall/CallHierarchyItem` | ✓ |
| `call_hierarchy_item_requires_name` | `CallHierarchyItem` 缺 `name`→`errMissing`（green-on-arrival） | 缺 name 对象 → `missing required properties: name` | `lsp_generated.go:CallHierarchyItem` | ✓ |
| `call_hierarchy_incoming_params_item_typed` | `CallHierarchyIncomingCallsParams.item` typed round-trip（green-on-arrival） | `{"item":{...}}` → `item.name` | `lsp_generated.go:CallHierarchyIncomingCallsParams` | ✓ |
| `create_file_options_typed` | **tracer（真 RED→GREEN）**：`CreateFile.options` typed `CreateFileOptions` + round-trip | `{"kind":"create","uri":...,"options":{"overwrite":true,"ignoreIfExists":false}}` → overwrite/ignoreIfExists | `lsp_generated.go:CreateFile/CreateFileOptions` | ✓ |
| `rename_file_options_typed` | `RenameFile.options` typed（green-on-arrival） | `{"kind":"rename",...,"options":{"overwrite":true}}` → overwrite | `lsp_generated.go:RenameFile` | ✓ |
| `delete_file_options_typed` | `DeleteFile.options` typed（`recursive`/`ignoreIfNotExists`；green-on-arrival） | `{"kind":"delete",...,"options":{"recursive":true,"ignoreIfNotExists":true}}` → 两字段 | `lsp_generated.go:DeleteFile/DeleteFileOptions` | ✓ |
| `file_options_default_serialize_empty` | §8.6：三个 file options `default→{}`（green-on-arrival） | `T::default()` → `"{}"`（3 类型） | omitzero 不变式 | ✓ |

> 既有 `null_rejected_hover_range` / `null_rejected_callhierarchy_incoming_params_item` / `null_rejected_callhierarchy_incoming_call_from` / `null_rejected_completionitem_insert_text_format` / `unmarshal_completion_item` / `roundtrip_initialize_params_null_process_id` / `null_accepted_initialize_root_uri` / `null_accepted_initialize_workspace_folders` / `null_accepted_initialize_process_id` / `null_rejected_textdocumentedit_edits` 在字段内层类型从 `serde_json::Value` 收紧为 typed `Option<T>`/`req T`/`reqnn` 后**全部不改即过**（`reqnn`/`optn` 语义 + `..Default::default()` 对内层类型透明）。给 `SymbolKind` 补 `Default` 是纯新增。

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| ~~`(*ClientCapabilities).Resolve()` 转换正确性~~ | ✅ 已落地（4 组全树 red→green） | 完成 |
| ~~`ServerCapabilities` typed serde~~ | ✅ 已落地（11 组高价值 provider red→green） | 完成 |
| ~~`ServerCapabilities` 深/稀有 provider 的 typed option 树（triple-union/executeCommand/onTypeFormatting/diagnostic/onAutoInsert 等）~~ | ✅ 已落地（21/22 provider typed；`boolean_or_options_or_registration!` 宏） | 完成 |
| ~~triple-union / `*OrRegistrationOptions` 的 **registration 变体** typed serde（`*RegistrationOptions`）~~ | ✅ 本轮已落地（registration-options base tree + 14 个 typed `*RegistrationOptions`，red→green） | 完成 |
| ~~`ServerCapabilities.workspace`（`WorkspaceOptions`）typed serde~~ | ✅ 本轮已落地（`WorkspaceOptions` 子树 red→green，清空 ServerCapabilities 最后一个 raw 槽） | 完成 |
| ~~`PatternOrRelativePattern` 的 **`RelativePattern` 对象变体** typed serde~~ | ✅ 本轮已落地（`WorkspaceFolder`/`WorkspaceFolderOrURI`/`RelativePattern` red→green；两变体全 typed） | 完成 |
| ~~请求/结果参数 + 通知类型的 concrete-typed raw-JSON 槽（Hover.contents / *.documentation / clientInfo / trace / rootPath / workspaceFolders / serverInfo / labelDetails / tags / insertTextMode / command / callHierarchy item / file options）~~ | ✅ **本轮**已落地（17 处收紧，red→green，+30 单测） | 完成 |
| `TextDocumentEdit.edits`（`[]TextEditOrAnnotatedTextEditOrSnippetTextEdit`）typed serde | 深 3 臂 union；`reqnn`（拒 null）已正确 | — blocked-by `AnnotatedTextEdit`/`SnippetTextEdit` 类型 |
| `*Data` carrier（`CompletionItem.data`/`InlayHint.data`/`CallHierarchyItem.data`）typed serde | typescript-go 内部 cookie carrier，低价值；缺/null/round-trip 已正确 | — blocked-by 生成器 pass |
| `Command.arguments` / `InitializationOptions` typed serde | **intentionally-any**（Go `[]any`/`LSPAny`）：保留 raw 才是 Go-faithful，不 over-type | — N/A（设计如此） |
| `CodeActionKindDocumentation`（`CodeActionOptions.documentation`）typed serde | proposed/稀有嵌套类型，`ServerCapabilities` provider 树里唯一剩余 raw-JSON DEFER | — blocked-by 生成器 pass |
| 新枚举 `String()` stringer（`TextDocumentSyncKind` 已含；其余） | resolved/server 树不全用；生成器 pass 落地完整枚举集时补 | P8 |
| resolved / 请求类型显式 `null` 字段的 Go `errNull` 精度 | 非线上收报关键路径，低优先 | — blocked-by 生成器 pass |
| Go 非指针字段（`support`/`tokenTypes` 等）的精确反序列化严格度 | 本轮统一建成 `Option<T>`；对 `resolve()` 等价 | — blocked-by 生成器 pass |
