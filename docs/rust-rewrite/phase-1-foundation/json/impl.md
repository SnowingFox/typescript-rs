# json: 实现方案（impl.md）

**crate**：`tsgo_json`　**目标**：对 `go-json-experiment/json`（v2 JSON）的薄封装，统一全仓的 JSON 序列化/反序列化默认项（默认允许非法 UTF-8、可选确定性输出、缩进控制），并 re-export 编解码器类型。
**依赖（crate）**：`serde` / `serde_json`（Rust 侧对应 v2 JSON 能力）。
**Go 源**：`internal/json/`（1 个非测试文件：`json.go` 101 行）

## 这个包是什么（业务说明）

typescript-go 的所有 JSON 交互（tsconfig 读取、LSP 消息、`--generateTrace`、API 边界）都走这一层薄封装而非直接用 `encoding/json`。理由：

1. **默认 `AllowInvalidUTF8(true)`**：TS 源码/配置可能含非法 UTF-8，序列化不应因此报错。包级变量 `allowInvalid` 在每次调用时被前置到 opts。
2. **统一缩进策略**：`MarshalIndent` 在 `prefix==""&&indent==""` 时跳过缩进选项（v2 里 WithIndent 隐含多行输出）。
3. **re-export 类型**：把 `jsontext.Value/Kind/Decoder/Encoder`、`json.UnmarshalerFrom/MarshalerTo` 以及 token 常量（`BeginObject` 等）统一暴露给上层，使上层不直接耦合第三方包名。

它本身几乎没有业务逻辑，是"配置 + 转发"层。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `go-json-experiment/json`（v2） | `serde` + `serde_json` | Rust 生态对应物；`#[derive(Serialize, Deserialize)]` 替代 Go 的 struct tag |
| `json.Options`（可变 opts） | builder/选项结构（如 `JsonOptions{ allow_invalid_utf8, deterministic, indent }`） | Go 用 `...json.Options` 变参；Rust 用配置 struct 或 `serde_json::ser::PrettyFormatter` 等 |
| `AllowInvalidUTF8(true)` 默认 | serde_json 默认即接受任意 `String`（已是 UTF-8） | **存疑偏离**：Rust `String` 必为有效 UTF-8，非法 UTF-8 需用 `Vec<u8>`/`bytes`。语义不完全对齐，见偏离 |
| `Deterministic(v)` | serde_json 的 `BTreeMap` / 排序键 或自定义序列化 | 确定性输出靠有序容器；map 用 `BTreeMap` 或 `IndexMap` |
| `MarshalIndent(in, prefix, indent)` | `serde_json::to_string_pretty` / 自定义 `PrettyFormatter` | prefix 在 serde_json 无直接对应，需自定义 Formatter；`prefix==""&&indent==""` 走紧凑 `to_string` |
| `jsontext.Decoder/Encoder`（流式 token） | `serde_json::{Deserializer, Serializer}` + `StreamDeserializer` | 流式 API；token 常量（BeginObject 等）对应 serde 的事件/`Value` 变体 |
| `jsontext.Value`（原始 JSON 文本） | `serde_json::value::RawValue` / `serde_json::Value` | `Value` 是已编码的 raw JSON；优先 `Box<RawValue>` |
| `io.Writer` / `io.Reader` | `std::io::Write` / `std::io::Read` | `MarshalWrite` / `UnmarshalRead` 对应 |

> **核心偏离提示**：Go 这层的存在价值之一是"容忍非法 UTF-8"，而 Rust 的 `String`/`serde_json` 默认就要求有效 UTF-8。这意味着移植时需决定：(a) 上游读入时清洗/替换非法字节（推荐，归 vfs/scanner）；或 (b) 在需要保真的边界用 `&[u8]` + 自定义。本包 impl.md 标 `// TODO(port)`，默认采用 (a) 并在 tests.md 记入 P10 兜底。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/json/json.go` | `internal/json/lib.rs`（basename `json` == crate 目录名 → `lib.rs`） | 封装 + re-export |

## 依赖白名单（本包新增的 crate）

- `serde`（`derive` feature）、`serde_json`。记到 `references/crate-map.md`（PORTING §10 已列）。
- 若需保真 raw JSON：`serde_json` 的 `raw_value` feature。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/json/json.go`）

- [x] `pub fn marshal<T: Serialize>(in_: &T, opts: JsonOptions) -> Result<Vec<u8>, Error>` — 默认前置 allow-invalid-utf8 语义　`// Go: json.go:Marshal`
- [x] `pub fn marshal_write<W: Write, T: Serialize>(w: &mut W, in_: &T, opts) -> Result<(), Error>`　`// Go: json.go:MarshalWrite`
- [ ] `pub fn marshal_encode<T: Serialize>(enc: &mut Serializer, in_: &T, opts) -> Result<(), Error>`　`// Go: json.go:MarshalEncode`（DEFER(phase-8) 流式）
- [x] `pub fn marshal_indent<T: Serialize>(in_: &T, prefix: &str, indent: &str) -> Result<Vec<u8>, Error>` — `prefix==""&&indent==""` 走紧凑 `marshal`，否则带缩进　`// Go: json.go:MarshalIndent`
- [ ] `pub fn marshal_indent_write<W: Write, T: Serialize>(w, in_, prefix, indent) -> Result<(),Error>` — 同上分支　`// Go: json.go:MarshalIndentWrite`
- [x] `pub fn unmarshal<T: DeserializeOwned>(in_: &[u8], opts) -> Result<T, Error>`　`// Go: json.go:Unmarshal`
- [ ] `pub fn unmarshal_decode<T: DeserializeOwned>(dec: &mut Deserializer, opts) -> Result<T, Error>`　`// Go: json.go:UnmarshalDecode`（DEFER(phase-8) 流式）
- [x] `pub fn unmarshal_read<R: Read, T: DeserializeOwned>(r: R, opts) -> Result<T, Error>`　`// Go: json.go:UnmarshalRead`
- [ ] `pub fn allow_duplicate_names(allow: bool) -> Opt`（选项构造）　`// Go: json.go:AllowDuplicateNames`（DEFER(phase-8)）
- [x] `pub fn deterministic(v: bool) -> Opt`　`// Go: json.go:Deterministic`（实现为 `marshal_deterministic`）
- [ ] `pub fn with_indent(indent: &str) -> Opt`　`// Go: json.go:WithIndent`
- [ ] `pub fn new_decoder<R: Read>(r: R) -> Deserializer<...>`　`// Go: json.go:NewDecoder`
- [ ] re-export 类型别名：`Value` / `Kind` / `Decoder` / `Encoder` / trait `UnmarshalerFrom`(→自定义) / `MarshalerTo`　`// Go: json.go`（type 块）
- [ ] re-export token 常量：`BEGIN_OBJECT`/`END_OBJECT`/`NULL`/`BEGIN_ARRAY`/`END_ARRAY`　`// Go: json.go`（var 块）

### Cargo / crate 接线

- [x] `internal/json/Cargo.toml`（`name = "tsgo_json"`，deps `serde` `serde_json`）
- [x] 根 `Cargo.toml` workspace members 追加
- [x] `lib.rs` re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `marshal` / `unmarshal` 往返（round-trip）一个简单 struct —— tracer bullet（红→绿）。
2. `marshal_indent` 的 `prefix/indent` 空与非空两条分支。
3. `deterministic` 输出键序稳定（map → 有序）。
4. 流式 `Decoder`/`Encoder` 与 token 常量（被 LSP 用，可推迟到接通 LSP 时补）。

## 与 Go 的已知偏离（divergence）

- **非法 UTF-8 容忍**：Go 默认 `AllowInvalidUTF8(true)`，Rust 默认不支持。处理策略见上文（推荐上游清洗），标 `// TODO(port)`。
- **`prefix` 缩进前缀**：serde_json 无内建 `IndentPrefix`，需自定义 `Formatter`。`MarshalIndent("","")` 紧凑分支可直接对齐。
- **选项变参 → 配置 struct**：Go `...json.Options` 在 Rust 改为 `JsonOptions` 结构 + builder，结构等价的允许偏离。
- **第三方包替换**：`go-json-experiment` → `serde_json`，行为以"相同输入产生相同 JSON 文本（紧凑/缩进模式）"为对齐目标，由 tests.md 的 round-trip + golden 兜底。

## 转交 / 推迟（DEFER）

- 流式 `Decoder`/`Encoder` 的完整 token API 主要被 LSP（P8）使用；本包先实现 `marshal`/`unmarshal` 主路径，流式 API 标 `// DEFER(phase-8)`。
