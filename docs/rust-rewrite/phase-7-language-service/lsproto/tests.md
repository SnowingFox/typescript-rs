# lsproto: 测试清单（tests.md）

> 本文件记录 `tsgo_lsproto` 本轮（`ResolvedClientCapabilities` 能力树）的测试。
> Go 侧 `lsp_json_test.go`/`lsp_test.go`/`baseproto_test.go` **无任何 resolved 类型 / `Resolve()` 的测试**（`Resolve()` 由 Go 内部生产、极少 (de)serialize）。故按 PORTING §8.6「每个公开类型至少一条行为级测试」自写，expected 取 Go `json:",omitzero"` 标签语义与 LSP spec 字面量。

**完成列图例**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模（resolved 部分）**：0 直接 Go 单测 → 行为由本轮自写 serde 测试 + P10 端到端兜底。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试文件 | 顶层测试函数数 |
|---|---|---|
| （无 resolved 对应 Go 测试） | `internal/lsp/lsproto/resolved_test.rs`（兄弟文件，`use super::*;`，由 `resolved.rs` 末尾 `#[cfg(test)] #[path="resolved_test.rs"] mod tests;` 挂载） | 23 |

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

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `(*ClientCapabilities).Resolve()` 转换正确性（指针树 → resolved） | blocked-by 指针版 `ClientCapabilities` 树未移植（`ClientCapabilities` 仍 open-object） | P8（生成器 pass） |
| 新枚举 `String()` stringer | resolved 树不使用；生成器 pass 落地完整枚举集时补 | P8 |
| resolved 类型显式 `null` 字段的 Go `errNull` 精度 | resolved 非线上收报类型，低优先 | — blocked-by 生成器 pass |
