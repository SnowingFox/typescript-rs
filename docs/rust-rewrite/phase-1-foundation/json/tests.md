# json: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例。

## 0 直接单测的情况

- Go 侧无 `*_test.go`：`internal/json` 是对 `go-json-experiment` 的薄封装，**无直接单测**；其行为由 **P10 conformance/fourslash parity** 兜底（真实 tsconfig 解析、LSP 消息往返、`--generateTrace` 输出对拍）。
- 本轮补充的行为级 Rust 测试（基于公开接口，expected 取自 Go v2 JSON 的确定输出 / JSON spec 已知值）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `marshal_compact_object` | 紧凑序列化键值对象 | `{a:1,b:"x"}` → `{"a":1,"b":"x"}` | json.go:Marshal（v2 紧凑无空格） | ✓ |
| `marshal_unmarshal_round_trip` | 往返一致 | struct → bytes → struct 等值 | json.go:Marshal/Unmarshal | ✓ |
| `marshal_indent_empty_is_compact` | `prefix==""&&indent==""` 走紧凑 | `MarshalIndent(v,"","")` == `Marshal(v)` | json.go:MarshalIndent（显式分支） | ✓ |
| `marshal_indent_two_spaces` | 两空格缩进多行输出 | `{a:1}` indent=`"  "` → `"{\n  \"a\": 1\n}"` | json.go:MarshalIndent | ✓ |
| `marshal_indent_prefix` | 带前缀缩进 | prefix=`"\t"` indent=`"  "` → 每行前置 `\t` | json.go:MarshalIndent（需自定义 Formatter） | ✓ |
| `deterministic_map_key_order` | 确定性输出键序稳定 | 同一 map 两次序列化字节相同且键有序 | json.go:Deterministic | ✓ |
| `unmarshal_into_struct` | 反序列化到结构体 | `{"x":3}` → `S{x:3}` | json.go:Unmarshal | ✓ |
| `marshal_write_to_writer` | 写入 `io::Write` | 写入 `Vec<u8>` 与 `marshal` 结果一致 | json.go:MarshalWrite | ✓ |

> 非法 UTF-8 容忍（Go 默认 `AllowInvalidUTF8`）不在 Rust 行为级单测覆盖（Rust `String` 必有效 UTF-8）；该差异的端到端影响归 P10 兜底，见下表。

## 与 impl.md 的对齐核对

- [x] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/json/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）

- [x] 无 Go `func Test*` 需映射（0 直接单测，已说明）
- [x] 补充测试覆盖 marshal/unmarshal/marshal_indent/deterministic/marshal_write 主路径，均在 impl.md 有 TODO 承载
- [x] expected 取自 v2 JSON 紧凑/缩进的确定输出（非 Rust 随意推断）
- [x] 每条补充测试标注依据

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 真实 tsconfig.json 解析与 Go 字节级一致 | 需 tsoptions（P6）落地 | P10 parity |
| LSP 消息流式 Decoder/Encoder 行为 | 需 LSP（P8）落地 | P8 / P10 |
| 非法 UTF-8 输入的容忍/降级行为与 Go 一致 | Rust 类型系统差异，需端到端语料 | P10 parity |
| `--generateTrace` 输出与 Go 字节级对拍 | 需 tracing（P6） | P10 parity |
