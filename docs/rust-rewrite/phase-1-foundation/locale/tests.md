# locale: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例。

## 0 直接单测的情况

- Go 侧无 `*_test.go`：`internal/locale` 是 29 行的薄包，**无直接单测**；其行为由 **P10 conformance/fourslash parity** 兜底（`--locale` 选项对诊断消息语言的端到端影响）。
- 本轮补充的行为级 Rust 测试（基于 BCP-47 已知值 + Go `Parse` 宽松失败语义）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `parse_valid_simple` | 简单语言标签 | `parse("en")` → Some | locale.go:Parse | |
| `parse_valid_region` | 语言-地区 | `parse("zh-CN")` → Some；`parse("ja")` → Some | locale.go:Parse | |
| `parse_invalid_returns_none` | 非法标签宽松失败 | `parse("not a locale!!")` → None | locale.go:Parse（ok=false） | |
| `parse_empty_returns_none_or_default` | 空串行为 | `parse("")` → None（与 Go `language.Parse("")` 一致的失败/und 处理） | locale.go:Parse | |
| `default_is_zero_value` | Default 为零值标签 | `Locale::default()` 等于零值 | locale.go:Default | |

> ⚠️ 注：`parse_empty` 的确切返回需在实现期对照 `unic-langid` 与 Go `x/text` 对空串/`"und"` 的处理；当前按"非法→None"标注，存疑见下表。

## 与 impl.md 的对齐核对

- [ ] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/locale/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）

- [x] 无 Go `func Test*` 需映射（0 直接单测，已说明）
- [x] 补充测试覆盖 `parse` 合法/非法/空 + `Default`，均在 impl.md 有 TODO 承载
- [x] expected 取自 BCP-47 已知值与 Go 宽松失败语义
- [x] 每条补充测试标注依据

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `--locale` 影响诊断消息语言（端到端） | 需 diagnostics（P2）落地 | P2 / P10 |
| `unic-langid` 与 `x/text/language` 解析宽松度逐标签对齐 | 需大规模标签语料 | P10 parity |
| 空串/`und`/私有子标签边界 | 两库行为差异需实测确认 | 实现期 + P10 |
