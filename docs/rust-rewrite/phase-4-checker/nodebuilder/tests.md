# nodebuilder: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：**0 个 `*_test.go`**。Go 侧该包无任何直接单测。

## 0 直接单测的情况

- Go 侧 `internal/nodebuilder/` 只有 `types.go`（纯类型/接口定义），无 `*_test.go`。
- 该包行为完全由其消费者覆盖：node builder 的真实逻辑在 **checker**（`nodebuilder*.go`，靠 P10 conformance/`.d.ts` 生成 parity 兜底），`SymbolTracker` 的回调路径由 declaration emit（P5）的 parity 覆盖。
- 因此本包归入 README "0 直接单测"清单，行为由 **P10 parity** 兜底。
- 本轮**补充**的行为级 Rust 测试只能覆盖"纯定义"的可验证部分：**bitflags 位值快照** + **trait 对象安全**。

## 补充行为级 Rust 测试（`lib.rs` 内 `#[cfg(test)] mod tests`）

### Flags / InternalFlags 位值快照（防止移植时位值错排）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `flags_bit_values` | 每个 `Flags` 常量整型值与 Go 一致 | `NoTruncation.bits()==1<<0`, `OmitParameterModifiers==1<<13`, `UseAliasDefinedOutsideCurrentScope==1<<14`, `OmitThisParameter==1<<25`, `AllowNodeModulesRelativePaths==1<<26`, `WriteCallStyleSignature==1<<27`, `UseSingleQuotesForStringLiteralType==1<<28`, `NoTypeReduction==1<<29`, `UseInstantiationExpressions==1<<30` | types.go:Flags 常量 | ✓ |
| `flags_state_bits` | 状态位 | `InObjectTypeLiteral==1<<22`, `InTypeAlias==1<<23`, `InInitialEntityName==1<<24` | types.go:Flags | ✓ |
| `flags_ignore_errors_composition` | `IGNORE_ERRORS` 组合成员正确（含 error 组各位 + 排除 `AllowUniqueESSymbolType`） | `IGNORE_ERRORS == AllowThisInObjectLiteral\|AllowQualifiedNameInPlaceOfIdentifier\|AllowAnonymousIdentifier\|AllowEmptyUnionOrIntersection\|AllowEmptyTuple\|AllowEmptyIndexInfoType\|AllowNodeModulesRelativePaths` | types.go:FlagsIgnoreErrors | ✓ |
| `internal_flags_bit_values` | InternalFlags 位值 | `WriteComputedProps==1<<0`, `NoSyntacticPrinter==1<<1`, `DoNotIncludeSymbolChain==1<<2`, `AllowUnresolvedNames==1<<3` | types.go:InternalFlags | ✓ |

### SymbolTracker trait（对象安全 + mock）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `symbol_tracker_object_safe` | trait 可作 `dyn`，mock 实现所有方法 | 构造 `Box<dyn SymbolTracker>` / `&mut dyn SymbolTracker` 调用全部 12 个方法（共 13 次）不 panic、`track_symbol` 返回 mock 设定值（true / false 两路） | types.go:SymbolTracker | ✓ |

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/nodebuilder/types.go:<Func>`，因 Go 侧无 `*_test.go`）

- [x] 每个 Go `func Test*` 都已映射 —— **N/A**（Go 侧 0 单测）
- [x] expected 值均取自 Go 源里的位值字面量
- [x] 每条对应 impl.md 的一个实现 TODO（`Flags`/`InternalFlags`/`SymbolTracker`）
- [x] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| node builder 真实序列化（Type→TypeNode）正确性 | 实现在 checker，需真类型系统 | P4 checker / P10 |
| `SymbolTracker` 各 report 回调在 `.d.ts` emit 中的触发 | 需 declaration emit 管线 | P5 / P10 |
