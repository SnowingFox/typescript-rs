# lsp: 测试清单（tests.md）

> 已实读全部 13 个 `*_test.go`（lsp 包 10 + lsproto 包 3），逐 `func Test*`、逐表驱动子用例对齐。
> **完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
> **Go 测试规模**：13 文件 / 42 顶层 `func Test*`（含 `TestMain`）/ 约 95 子用例。
> crate 拆分：`tsgo_lsproto`（协议）+ `tsgo_lsp`（服务器）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `lsproto/baseproto_test.go` | `tsgo_lsproto` `baseproto.rs`（`#[cfg(test)] mod tests`） | 4 |
| `lsproto/lsp_test.go` | `tsgo_lsproto` `message.rs`/`generated.rs` tests | 1 |
| `lsproto/lsp_json_test.go` | `tsgo_lsproto` `generated.rs` tests | 20 |
| `lsp/progress_test.go` | `tsgo_lsp` `progress.rs` tests | 1（11 子用例） |
| `lsp/stack_sanitizer_test.go` | `tsgo_lsp` `stack_sanitizer.rs` tests（baseline 对拍） | 3 |
| `lsp/replay_test.go` | P10（lsptestutil 就绪后） | 1 |
| `lsp/server_completion_test.go` | `tests/server_completion.rs`（集成） | 5 |
| `lsp/server_progress_test.go` | `tests/server_progress.rs` | 1 |
| `lsp/server_projectinfo_test.go` | `tests/server_projectinfo.rs` | 2 |
| `lsp/server_projectreference_updates_test.go` | `tests/server_projectreference_updates.rs` | 1 |
| `lsp/server_semantictokens_test.go` | `tests/server_semantictokens.rs` | 1 |
| `lsp/server_shutdown_test.go` | `tests/server_shutdown.rs` | 1 |
| `lsp/testmain_test.go` | test harness（`TestMain`，无业务断言） | 1 |

---

## `lsproto/baseproto_test.go`（→ 同时是 `tsgo_jsonrpc` 的 ground truth，见 `../jsonrpc/tests.md`）

### `TestBaseReader`（表驱动，9 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `base_reader_empty` | 长度 0 → 无内容错误 | `"Content-Length: 0\r\n\r\n"` → err `"jsonrpc: no content length"` | `TestBaseReader/empty` | |
| `base_reader_early_end` | 提前 EOF | `"oops"` → err `"EOF"` | `.../early end` | |
| `base_reader_negative_length` | 负长度 | `"Content-Length: -1\r\n\r\n"` → err `"jsonrpc: invalid content length: negative value -1"` | `.../negative length` | |
| `base_reader_invalid_content` | 长度 1 读 `{` | `"Content-Length: 1\r\n\r\n{"` → `b"{"` | `.../invalid content` | |
| `base_reader_valid_content` | 标准 | `"Content-Length: 2\r\n\r\n{}"` → `b"{}"` | `.../valid content` | |
| `base_reader_extra_header_values` | 忽略额外 header | `"Content-Length: 2\r\nExtra: 1\r\n\r\n{}"` → `b"{}"` | `.../extra header values` | |
| `base_reader_too_long_content_length` | 长度过大 | `"Content-Length: 100\r\n\r\n{}"` → err `"jsonrpc: read content: unexpected EOF"` | `.../too long content length` | |
| `base_reader_missing_content_length` | 空数值 | `"Content-Length: \r\n\r\n{}"` → err 含 `"...parse error: strconv.ParseInt: parsing \"\": invalid syntax"` | `.../missing content length` | |
| `base_reader_invalid_header` | 无冒号 header | `"Nope\r\n\r\n{}"` → err `"jsonrpc: invalid header: \"Nope\\r\\n\""` | `.../invalid header` | |

### `TestBaseReaderMultipleReads`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `base_reader_multiple_reads` | 连读两条后 EOF | `"Content-Length: 4\r\n\r\n1234Content-Length: 2\r\n\r\n{}"` → `b"1234"`,`b"{}"`,`EOF` | `TestBaseReaderMultipleReads` | |

### `TestBaseWriter`（表驱动，2 子用例）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `base_writer_empty` | 写 `{}` | `b"{}"` → `"Content-Length: 2\r\n\r\n{}"` | `TestBaseWriter/empty` | |
| `base_writer_bigger_object` | 写键值对象 | `b"{\"key\":\"value\"}"` → `"Content-Length: 15\r\n\r\n{\"key\":\"value\"}"` | `.../bigger object` | |

### `TestBaseWriterWriteError`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `base_writer_write_error` | 透传底层 IO 错误 | writer 总报错 → `write` 返回 `"test error"` | `TestBaseWriterWriteError` | |

---

## `lsproto/lsp_test.go`

### `TestUnmarshalCompletionItem`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `unmarshal_completion_item` | 复杂 CompletionItem 解码（含 InsertReplaceEdit 联合、kind=6→Variable、insertTextFormat=1→PlainText、commitCharacters 数组） | 见 Go 中 `message` JSON → `CompletionItem{Label:"pageXOffset", InsertTextFormat:PlainText, TextEdit:InsertReplaceEdit{NewText:"pageXOffset", Insert/Replace: Range(0..4@line4)}, Kind:Variable, SortText:"15", CommitCharacters:[".",",",";"]}` | `lsp_test.go:TestUnmarshalCompletionItem` | |

---

## `lsproto/lsp_json_test.go`（serde 行为核心，20 函数）

### `TestUnmarshalRejectsNullForOptionalNonNullableFields`（14 子用例）

> 行为：optional 但 non-nullable 字段收到 `null` → 报 `null value is not allowed for field "<f>"`。

| Rust 测试 | input → expected err | Go 对照 | 完成 |
|---|---|---|---|
| `null_rejected_inlayhint_kind` | `{...,"kind":null}` → `null value is not allowed for field "kind"` | `.../InlayHint kind null` | |
| `null_rejected_inlayhint_text_edits` | `"textEdits":null` → `..."textEdits"` | `.../InlayHint textEdits null` | |
| `null_rejected_inlayhint_padding_left` | `"paddingLeft":null` → `..."paddingLeft"` | `.../InlayHint paddingLeft null` | |
| `null_rejected_foldingrange_kind` | `"kind":null` → `..."kind"` | `.../FoldingRange kind null` | |
| `null_rejected_foldingrange_start_character` | `"startCharacter":null` → `..."startCharacter"` | `.../FoldingRange startCharacter null` | |
| `null_rejected_completionitem_insert_text_format` | `"insertTextFormat":null` → `..."insertTextFormat"` | `.../CompletionItem insertTextFormat null` | |
| `null_rejected_hover_range` | `"range":null` → `..."range"` | `.../Hover range null` | |
| `null_rejected_workdoneprogressoptions_work_done_progress` | `"workDoneProgress":null` → `..."workDoneProgress"` | `.../WorkDoneProgressOptions workDoneProgress null` | |
| `null_rejected_callhierarchy_incoming_params_item` | `"item":null` → `..."item"` | `.../CallHierarchyIncomingCallsParams item null` | |
| `null_rejected_callhierarchy_incoming_call_from` | `"from":null` → `..."from"` | `.../CallHierarchyIncomingCall from null` | |
| `null_rejected_initialize_params_capabilities` | `"capabilities":null` → `..."capabilities"` | `.../InitializeParams capabilities null` | |
| `null_rejected_initialize_result_capabilities` | `"capabilities":null` → `..."capabilities"` | `.../InitializeResult capabilities null` | |
| `null_rejected_semantictokens_data` | `"data":null`（required slice）→ `..."data"` | `.../SemanticTokens data null` | |
| `null_rejected_textdocumentedit_edits` | `"edits":null`（required slice）→ `..."edits"` | `.../TextDocumentEdit edits null` | |

### `TestUnmarshalAcceptsNullForNullableFields`（4 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `null_accepted_initialize_root_uri` | `{processId:null, rootUri:null, capabilities:{}}` → Ok | `.../InitializeParams rootUri null` | |
| `null_accepted_initialize_workspace_folders` | `+ workspaceFolders:null` → Ok | `.../InitializeParams workspaceFolders null` | |
| `null_accepted_initialize_process_id` | `processId:null` → Ok | `.../InitializeParams processId null` | |
| `null_accepted_initialization_options_user_preferences` | `{userPreferences:null}` → Ok | `.../InitializationOptions userPreferences null` | |

### `TestUnmarshalAcceptsOmittedOptionalFields`（2 子用例 + 字段断言）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `omitted_optional_inlayhint` | `{position:{1,5}, label:"test"}` → kind/textEdits/tooltip/paddingLeft/paddingRight/data 全 None；position=(1,5) | `.../InlayHint with only required fields` | |
| `omitted_optional_foldingrange` | `{startLine:5, endLine:10}` → kind/startChar/endChar/collapsedText 全 None；start=5,end=10 | `.../FoldingRange with only required fields` | |

### `TestUnmarshalRejectsIncompleteObjects`（4 子用例）

| Rust 测试 | input → expected err | Go 对照 | 完成 |
|---|---|---|---|
| `incomplete_inlayhint_missing_position` | `{label:"test"}` → `missing required properties: position` | `.../InlayHint missing position` | |
| `incomplete_inlayhint_missing_label` | `{position:{0,0}}` → `missing required properties: label` | `.../InlayHint missing label` | |
| `incomplete_location_missing_uri` | `{range:{...}}` → `missing required properties: uri` | `.../Location missing uri` | |
| `incomplete_location_empty` | `{}` → `missing required properties: uri, range` | `.../Location empty object` | |

### `TestMarshalUnmarshalRoundTrip`（5 子用例）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `roundtrip_inlayhint_with_kind` | `InlayHint{pos(1,5),label:"param",kind:Parameter}` marshal→unmarshal 相等 | `.../InlayHint with kind` | |
| `roundtrip_inlayhint_minimal` | `InlayHint{pos(0,0),label:"x"}` | `.../InlayHint minimal` | |
| `roundtrip_foldingrange_all` | `FoldingRange{1,start0,end10,end5,Region,"..."}` | `.../FoldingRange with all fields` | |
| `roundtrip_location` | `Location{file:///test.ts, range(1,2)-(3,4)}` | `.../Location` | |
| `roundtrip_initialize_params_null_process_id` | `InitializeParams{ProcessId:null,RootUri:file:///workspace,Capabilities:{}}` | `.../InitializeParams with null processId` | |

### `TestUnmarshalUnionTypes`（6 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `union_integer_or_string_int` | `42` → Integer(42), String None | `.../IntegerOrString with integer` | |
| `union_integer_or_string_str` | `"hello"` → String("hello"), Integer None | `.../IntegerOrString with string` | |
| `union_integer_or_null_int` | `42` → Integer(42) | `.../IntegerOrNull with integer` | |
| `union_integer_or_null_null` | `null` → Integer None | `.../IntegerOrNull with null` | |
| `union_document_uri_or_null_str` | `"file:///test.ts"` → DocumentUri | `.../DocumentUriOrNull with string` | |
| `union_document_uri_or_null_null` | `null` → None | `.../DocumentUriOrNull with null` | |

### `TestMarshalUnionTypes`（4 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `marshal_integer_or_null_value` | `IntegerOrNull{42}` → `"42"` | `.../IntegerOrNull with value` | |
| `marshal_integer_or_null_null` | `IntegerOrNull{}` → `"null"` | `.../IntegerOrNull with null` | |
| `marshal_integer_or_string_int` | `IntegerOrString{7}` → `"7"` | `.../IntegerOrString with integer` | |
| `marshal_integer_or_string_str` | `IntegerOrString{"tok"}` → `"\"tok\""` | `.../IntegerOrString with string` | |

### `TestUnmarshalIgnoresUnknownFields`（2 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `ignore_unknown_location` | Location + `someUnknownField`/`anotherUnknown` → Ok, uri 正确 | `.../Location with extra fields` | |
| `ignore_unknown_inlayhint` | InlayHint + `futureField:[1,2,3]` → Ok | `.../InlayHint with extra fields` | |

### `TestUnmarshalRejectsWrongTypes`（5 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `wrong_type_location_array` | `[]`→Location err | `.../Location receives array` | |
| `wrong_type_location_string` | `"not an object"`→err | `.../Location receives string` | |
| `wrong_type_location_number` | `42`→err | `.../Location receives number` | |
| `wrong_type_location_null` | `null`→err | `.../Location receives null` | |
| `wrong_type_foldingrange_bool` | `true`→FoldingRange err | `.../FoldingRange receives boolean` | |

### `TestUnmarshalUnionTypeWrongKind`（6 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `union_wrong_int_or_string_bool` | `true`→err | `.../IntegerOrString rejects boolean` | |
| `union_wrong_int_or_string_null` | `null`→err | `.../IntegerOrString rejects null` | |
| `union_wrong_int_or_string_object` | `{}`→err | `.../IntegerOrString rejects object` | |
| `union_wrong_int_or_string_array` | `[]`→err | `.../IntegerOrString rejects array` | |
| `union_wrong_string_or_parts_number` | `42`→err | `.../StringOrInlayHintLabelParts rejects number` | |
| `union_wrong_string_or_parts_bool` | `true`→err | `.../StringOrInlayHintLabelParts rejects boolean` | |

### `TestUnmarshalBooleanUnionTypes`（4 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `bool_union_true` | `true` → Boolean(true), Options None | `.../BooleanOrHoverOptions with true` | |
| `bool_union_false` | `false` → Boolean(false) | `.../BooleanOrHoverOptions with false` | |
| `bool_union_object` | `{}` → Boolean None, Options Some | `.../BooleanOrHoverOptions with object` | |
| `bool_union_rejects_string` | `"nope"` → err | `.../BooleanOrHoverOptions rejects string` | |

### `TestUnmarshalDiscriminatorUnion`（4 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `discriminator_begin` | `{kind:"begin",title:"Indexing"}` → Begin{title:"Indexing"} | `.../WorkDoneProgressBegin` | |
| `discriminator_report` | `{kind:"report",message:"50%"}` → Report{message:"50%"} | `.../WorkDoneProgressReport` | |
| `discriminator_end` | `{kind:"end"}` → End | `.../WorkDoneProgressEnd` | |
| `discriminator_invalid` | `{kind:"invalid"}` → err | `.../invalid discriminator` | |

### `TestUnmarshalPresenceDiscriminatorUnion`（2 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `presence_text_edit_via_range` | 有 `range`+`newText` → TextEdit{newText:"x"} | `.../TextEdit via range field` | |
| `presence_insert_replace_via_insert` | 有 `insert`+`replace`+`newText` → InsertReplaceEdit{newText:"y"} | `.../InsertReplaceEdit via insert field` | |

### `TestUnmarshalStringOrArrayUnion`（2 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `string_or_array_string` | `"hello"` → String("hello") | `.../StringOrInlayHintLabelParts with string` | |
| `string_or_array_array` | `[{value:"param"},{value:": "},{value:"string"}]` → Parts(len 3, [0].value="param") | `.../StringOrInlayHintLabelParts with array` | |

### `TestUnmarshalDocumentEditUnion`（4 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `doc_edit_text_document_edit` | 无 kind + textDocument+edits → TextDocumentEdit | `.../TextDocumentEdit without kind` | |
| `doc_edit_create` | `{kind:"create",uri}` → CreateFile{uri} | `.../CreateFile with kind create` | |
| `doc_edit_rename` | `{kind:"rename",oldUri,newUri}` → RenameFile{oldUri} | `.../RenameFile with kind rename` | |
| `doc_edit_delete` | `{kind:"delete",uri}` → DeleteFile{uri} | `.../DeleteFile with kind delete` | |

### `TestUnmarshalFieldOrdering`（2 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `field_order_location_reversed` | range 在 uri 前 → 仍正确解析 | `.../Location with reversed field order` | |
| `field_order_inlayhint_kind_first` | `kind:1` 在前 → Kind=Type | `.../InlayHint with kind before label` | |

### `TestUnmarshalEmptyObject`（4 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `empty_workdoneprogressoptions` | `{}` → workDoneProgress None | `.../WorkDoneProgressOptions empty` | |
| `empty_initialization_options` | `{}` → Ok | `.../InitializationOptions empty` | |
| `empty_client_capabilities` | `{}` → Ok | `.../ClientCapabilities empty` | |
| `empty_server_capabilities` | `{}` → Ok | `.../ServerCapabilities empty` | |

### `TestMarshalOmitsZeroOptionalFields`（2 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `marshal_omits_inlayhint` | InlayHint 最小 → 输出含 position/label，不含 kind/textEdits/paddingLeft | `.../InlayHint omits nil fields` | |
| `marshal_omits_foldingrange` | FoldingRange{1,10} → 含 startLine/endLine，不含 kind/startCharacter | `.../FoldingRange omits nil optional fields` | |

### `TestLiteralTypes`（4 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `literal_create_marshal` | `StringLiteralCreate{}` → `"\"create\""` | `.../StringLiteralCreate marshal` | |
| `literal_create_unmarshal` | `"create"` → Ok | `.../StringLiteralCreate unmarshal` | |
| `literal_create_rejects_wrong_value` | `"delete"` → err | `.../StringLiteralCreate rejects wrong value` | |
| `literal_create_rejects_wrong_type` | `42` → err | `.../StringLiteralCreate rejects wrong type` | |

### `TestEnumStringValues`（3 子用例）

| Rust 测试 | input → expected | Go 对照 | 完成 |
|---|---|---|---|
| `enum_inlayhintkind_strings` | `Type.to_string()=="Type"`, `Parameter=="Parameter"` | `.../InlayHintKind values` | |
| `enum_symbolkind_strings` | `File=="File"`,`Function=="Function"`,`Variable=="Variable"` | `.../SymbolKind values` | |
| `enum_unknown_value` | `InlayHintKind(999).to_string()` 含 `"999"` | `.../unknown enum value` | |

---

## `lsp/progress_test.go`

### `TestProgress`（11 子用例，用 fake reporter + 可注入时钟）

> Rust：`progressReporter` → trait + `FakeProgressReporter`（记录 `Vec<ProgressCall>`）。`testing/synctest` 虚拟时钟 → 可注入 `Clock` trait（手动 `advance(d)`）+ `wait()`（让 run 线程处理完）。`diagnostics.Project_0` = `"Project '{0}'"`，`diagnostics.Loading` = `"Loading"`。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `progress_start_finish_before_delay` | 快操作（delay 前完成）→ 无任何调用 | start+finish+advance600ms（delay=500） → calls 空 | `TestProgress/StartFinishBeforeDelay` | |
| `progress_shows_after_delay` | delay 后显示 create+begin（title="Loading"），finish→end | start+advance500 → [create,begin(title=Loading)]；finish → 末尾 end | `.../ShowsAfterDelay` | |
| `progress_reports_multiple_operations` | 两操作：create+begin；finish 一个→report；finish 另一→end | startA,startB,advance100 → ≥[create,begin]；finishA→出现 report；finishB→end | `.../ReportsMultipleOperations` | |
| `progress_ref_counting` | 同操作 start×2，finish×1 不 end，finish×2 才 end | start,start,advance100,finish→无 end；finish→end | `.../RefCounting` | |
| `progress_new_token_after_end` | 第二轮拿到不同 token | 第一轮 token≠第二轮 token | `.../NewTokenAfterEnd` | |
| `progress_start_before_delay_then_more_after` | delay 后新 start → 立即 report | startA,advance200(≥create+begin),startB → 末尾 report | `.../StartBeforeDelayThenMoreAfterDelay` | |
| `progress_finish_with_no_active_token` | 无 start 直接 finish → no-op | finish → calls 空 | `.../FinishWithNoActiveToken` | |
| `progress_shutdown_during_start_and_finish` | ctx 取消 + channel 满 → start/finish 立即返回（done 路径） | cancel，填满 ch，start/finish 不阻塞 | `.../ShutdownDuringStartAndFinish` | |
| `progress_shutdown_with_active_timer` | 延迟 timer pending 时 shutdown → 干净退出 | start(delay=500),cancel → 无 panic/leak | `.../ShutdownWithActiveTimer` | |
| `progress_zero_delay` | delay=0 → 立即 create+begin（msg="Project 'proj'"），finish→end | start → [create,begin(msg="Project 'proj'")]；finish→end | `.../ZeroDelay` | |
| `progress_finish_before_delay_no_begun` | begun=false 时 finish → 不发 end | start,finish（delay 前）→ 无 end | `.../FinishBeforeDelayNoBegun` | |

---

## `lsp/stack_sanitizer_test.go`（baseline 对拍，3 函数）

> 纯函数，输入为完整 Go panic 栈字符串，输出为净化后字符串。Rust 用快照测试（`insta` 或内联 baseline 文件），expected = Go baseline 内容。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `sanitize_debug_stack_trace_completions` | debug 构建（绝对路径）栈净化：去非本仓帧→`(REDACTED FRAME)`，本仓帧去参数/`+0x`，`/`→`\|>` | 见 Go 内 debug `input` → baseline `completionsDebugStackTrace.md` | `TestSanitizedDebugStackTraceCompletionsRequest` | |
| `sanitize_release_stack_trace_completions` | release 构建（相对路径）栈净化 | 见 Go 内 release `input` → baseline `completionsReleaseStackTrace.md` | `TestSanitizedReleaseStackTraceCompletionsRequest` | |
| `sanitize_defeats_vscode_generic_secret_regex` | 净化后不被 VS Code `(key\|token\|sig\|secret\|signature\|password\|passwd\|pwd\|android:value)[^a-zA-Z0-9]` 命中（插入 `X_X`） | 含 getSignatureHelp/LookupKey/validateToken/signRequest/setPwd 的栈 → 正则不命中 + baseline `genericSecretWorkaround.md` | `TestSanitizedStackTraceDefeatsVSCodeGenericSecretRegex` | |

---

## `lsp/server_*_test.go`（集成测试，依赖 project+ls+lsptestutil）

> Rust 用集成测试（`tests/`）+ `lsptestutil`（P10）等价物。大部分 `// DEFER`：blocked-by `tsgo_project`/`tsgo_ls`/`tsgo_api` green。`bundled.Embedded` 守卫 → Rust 同样 skip 非 embedded 构建。

### `server_completion_test.go`（5 函数）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `completion_after_file_close` | 文件已关闭后仍能补全（someVar，auto-import `./a`） | open a/b,close b,completion@(0,1) → item someVar, AutoImport.ModuleSpecifier=="./a" | `TestCompletionAfterFileClose` | —(blocked) |
| `completion_with_concurrent_file_close` | 补全请求先入队再 close → 仍成功 | async completion + close → someVar, "./a" | `TestCompletionWithConcurrentFileClose` | —(blocked) |
| `completion_for_unopened_file` | 未打开文件补全 | completion@(1,2) on c.ts → 含 xyz | `TestCompletionForUnopenedFile` | —(blocked) |
| `auto_import_completion_for_unopened_file` | 未打开文件 auto-import 补全 | completion@(0,1) → someVar, "./a" | `TestAutoImportCompletionForUnopenedFile` | —(blocked) |
| `completion_snapshot_freezing` | auto-import 重试用同步阶段快照（并发 DidChange 不影响） | async completion@(0,5) + DidChange("notMatching") → 仍含 someVar, "./a" | `TestCompletionSnapshotFreezing` | —(blocked) |

### `server_progress_test.go`（1 函数）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `progress_notifications_end_to_end` | 端到端 `$/progress`：首=begin(title="Loading")，末=end，全部同 token | init(WorkDoneProgress)+didOpen+projectInfo → ≥2 通知，begin/end，token 一致 | `TestProgressNotificationsEndToEnd` | —(blocked) |

### `server_projectinfo_test.go`（2 函数）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `project_info_configured_project` | configured 项目返回 tsconfig 路径 | tsconfig.json + index.ts → ConfigFilePath=="/home/projects/tsconfig.json" | `TestProjectInfoConfiguredProject` | —(blocked) |
| `project_info_inferred_project` | inferred 项目返回空路径 | 仅 index.ts → ConfigFilePath=="" | `TestProjectInfoInferredProject` | —(blocked) |

### `server_projectreference_updates_test.go`（1 函数）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `references_after_ancestor_project_config_deletion` | 删除祖先 tsconfig 后引用查找仍返回 2 处 | 删 root/tsconfig.json+watched-file delete，references@(1,3) → 2 Locations（(0,16)-(0,26),(1,0)-(1,10)） | `TestReferencesAfterAncestorProjectConfigDeletion1` | —(blocked) |

### `server_semantictokens_test.go`（1 函数）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `semantic_tokens_crlf` | CRLF overlay vs LF on-disk 不再 panic "token spans multiple lines" | LF 加载+CRLF 打开，semanticTokens/full → Error==nil | `TestSemanticTokensCRLF` | —(blocked) |

### `server_shutdown_test.go`（1 函数）

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `server_shutdown_no_deadlock` | shutdown 后操作不阻塞（outgoingQueue 填满也不死锁） | cancel ctx+填满队列+DidChange/GetLanguageService → 不阻塞 | `TestServerShutdownNoDeadlock` | —(blocked) |

### `replay_test.go`（1 函数）

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `replay`（CLI flag 驱动的 replay） | 回放 LSP 消息序列 + 占位符替换，无错误 | `TestReplay` | —(P10) |

### `testmain_test.go`

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| （test harness：`ApplyDebugStackLimit` + baseline `Track`） | 无业务断言，Rust 用 `#[ctor]`/baseline harness 等价 | `TestMain` | —(harness) |

---

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（42 个，含 TestMain）都已映射
- [x] 每个表驱动子用例都已逐行列出（baseproto 9+1+2+1、progress 11、lsp_json 各组共 ~70）
- [x] expected 值均取自 Go 测试字面量（错误文案、JSON 字面量、断言值）
- [x] 每条带 `// Go:` 锚点（Go 对照列）
- [x] 与 impl.md 双向对齐：lsproto 的 (de)serialize / progress 状态机 / stack_sanitizer / server handler 均有实现 TODO 承载

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 全部 `server_*_test.go`（completion/progress-e2e/projectinfo/projectref/semantictokens/shutdown） | 依赖 `tsgo_project`+`tsgo_ls`+`tsgo_api`+`lsptestutil` | P8 末尾收口 / 部分 P10 |
| `TestReplay` | 依赖 `testutil/lsptestutil` + replay 设施 | P10 |
| `lsp_generated.rs` 全量类型的解码测试 | 生成器就绪后全量 parity | P10 conformance |
