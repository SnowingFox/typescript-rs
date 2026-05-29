# diagnostics: 实现方案（impl.md）

**crate**：`tsgo_diagnostics`　**目标**：承载全套**可本地化的诊断消息**（~2153 条），提供按 locale 取译文 + `{0}/{1}` 占位符格式化的运行时；是 parser / binder / checker / ls 报错时的消息字典。
**依赖（crate）**：`tsgo_core`（`SameMap` 等工具、`ScriptKind` 无关）、`tsgo_locale`（`Locale` = BCP-47 标签包装）、`tsgo_json`（解压后反序列化 locale 数据）。镜像 Go import：`internal/core`、`internal/locale`、`internal/json`、`golang.org/x/text/language`。
**Go 源**：`internal/diagnostics/`（5 个非测试 `.go` 文件 + `loc/` 子目录 14 个 `*.json.gz` + `extraDiagnosticMessages.json` + `generate.go` 代码生成器）

## 这个包是什么（业务说明）

`diagnostics` 是 TypeScript 编译器所有报错文案的**唯一来源**。每条诊断（如 `error TS1005: ')' expected.`）背后是一个 `Message{ code, category, key, text }`。编译器各阶段不直接拼字符串，而是引用包级变量（如 `diagnostics.Identifier_expected`）再调用 `.Localize(locale, args...)` 得到最终文案。

它做三件事：

1. **消息字典**（`diagnostics_generated.go`，~2153 个 `var`）：每个 `*Message` 是不可变单例，编译期就固定 `code/category/key/text`。这些由 `generate.go` 从 TypeScript 子模块的 `src/compiler/diagnosticMessages.json` + 本包的 `extraDiagnosticMessages.json`（CLI/LS 专属、code≥100000 的额外消息）生成。
2. **本地化**（`diagnostics.go` + `loc_generated.go` + `loc/*.json.gz`）：14 种语言的译文以 gzip+JSON 嵌入二进制，按请求 locale 用 `language.Matcher` 匹配最近语言，懒解压（`sync.OnceValue`）并缓存（`sync.Map`）。匹配不到或英语回落到 `Message.text`。
3. **占位符格式化**（`Format`）：把 `{0}` `{1}` 替换成参数，参数先经 `ToValidUTF8` 清洗。

它在 Phase 2 落地，因为 `ast` 包的 `Diagnostic`（见 ast/impl.md `diagnostic.rs`）直接引用 `diagnostics.Message` / `diagnostics.Category` / `diagnostics.Key`，是 AST 的前置依赖。本包**自身不依赖 ast**，是叶子。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type Category int32` + `iota` | `#[repr(i32)] enum Category { Warning, Error, Suggestion, Message }` | 4 个值，顺序即数值（Warning=0…Message=3），须与 Go 一致以保 code 序 |
| `func (Category) Name() string` | `impl Category { pub fn name(&self) -> &'static str }` | 返回 `"warning"/"error"/"suggestion"/"message"`；非法值 Go `panic`，Rust `unreachable!`（enum 已穷尽，天然不可达） |
| `Category.String()`（stringer） | `impl Display for Category` 或 `fn debug_name()` | stringer 产物返回 `"CategoryWarning"` 等；仅调试用，可用 `#[derive(Debug)]` 近似 |
| `type Key string` | `pub struct Key(&'static str)` 或 `type Key = &'static str` | key 是 `"_0_expected_1005"` 形态的稳定标识；生成的消息全用 `&'static str`，故 `&'static str` 足够 |
| `type Message struct{ code int32; category Category; key Key; text string; reportsUnnecessary bool; elidedInCompatibilityPyramid bool; reportsDeprecated bool }` | `pub struct Message { code: i32, category: Category, key: Key, text: &'static str, reports_unnecessary: bool, elided_in_compatibility_pyramid: bool, reports_deprecated: bool }` | 字段全私有（Go 是小写），用 getter 暴露。生成的实例全为 `&'static`（编译期常量） |
| `var Identifier_expected = &Message{...}`（2153 个包级指针） | `pub static IDENTIFIER_EXPECTED: Message = Message { .. };`（生成）→ 引用为 `&'static Message` | Go 用堆上单例指针；Rust 用 `static`，零运行时分配。命名见下方"命名"小节 |
| `keyToMessage(key Key) *Message`（生成的大 switch） | `pub fn key_to_message(key: &str) -> Option<&'static Message>`（生成，`match` 或 `phf`/`OnceLock<HashMap>`） | 2153 条；用编译期完美哈希 `phf` 或一次性构建的 `HashMap` |
| `map[Key]string` 译文表 | `FxHashMap<Key, String>`（解压后） | 无序即可（按 key 查找，不影响输出顺序） |
| `sync.Map`（locale 缓存） | `OnceLock<RwLock<FxHashMap<Tag, Option<Arc<Map>>>>>` 或 `dashmap::DashMap` | 并发读多写少；译文表用 `Arc` 共享 |
| `sync.OnceValue(func() map[Key]string)` | `OnceLock<FxHashMap<Key,String>>` + 加载闭包 | 每语言懒解压一次 |
| `//go:embed loc/zh-CN.json.gz` → `var zhCNData string` | `static ZH_CN_DATA: &[u8] = include_bytes!("loc/zh-CN.json.gz");` | `include_bytes!` 对应 `go:embed`；数据是 gzip 字节 |
| `language.Matcher` / `language.Tag` | 见下"locale 匹配"——P2 用简化匹配器 + `// TODO(port)` | `golang.org/x/text/language` 无直接 Rust 等价；需自实现 BCP-47 最近匹配或引 `oxilangtag`/`icu` |
| `regexp.MustCompile(`{(\d+)}`)` | `OnceLock<Regex>`（`regex` crate） | 占位符提取；也可手写扫描避免 regex 依赖 |
| `core.SameMap` | `tsgo_core` 对应的 map-in-place | 见 Phase 1 core |

### locale 匹配（本包最大偏离点，须重点说明）

Go 用 `golang.org/x/text/language` 的 `NewMatcher` + `Match`（CLDR 语言距离算法）把任意请求 locale（如 `af-ZA`）匹配到 14 个支持语言中的最近者（匹配不到则英语回落，`confidence >= Low`）。Rust 标准库与主流 crate 无逐一对齐的 CLDR matcher。决策：

- **P2 落地**：实现一个**精简匹配器**——先精确匹配 `tag`，再退到 base language（`zh-CN` ⊃ `zh`、`zh-TW`），匹配不到回落英语（返回 `None` → 用 `Message.text`）。覆盖测试里出现的所有 case（`de-DE`/`fr-FR`/`es-ES`/`ja-JP`/`zh-CN`/`ko-KR`/`ru-RU` 精确命中；`af-ZA`/`Und` 回落英语）。标 `// TODO(port): 完整 CLDR 语言距离匹配，blocked-by: 选定 icu/oxilangtag`。
- **类型**：`Locale` 来自 `tsgo_locale`，内部包 BCP-47 标签。P2 先以"规范化小写字符串 + base 截断"为键，足够通过单测；端到端 locale 行为由 P10 兜底。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/diagnostics/diagnostics.go` | `internal/diagnostics/diagnostics.rs`（crate 根 → `lib.rs`） | `Category` + `Message` + getter + `Localize`/`Format`/`StringifyArgs` + locale 缓存与匹配；crate 入口 |
| `internal/diagnostics/diagnostics_generated.go` | `internal/diagnostics/diagnostics_generated.rs` | 生成：~2153 个 `static Message` + `key_to_message`。**不手写**，由移植版生成器产出（见下） |
| `internal/diagnostics/loc_generated.go` | `internal/diagnostics/loc_generated.rs` | 生成：14 语言的 `include_bytes!` + 懒加载 + matcher 表 |
| `internal/diagnostics/stringer_generated.go` | （并入 `diagnostics.rs`） | `Category` 的 `Display`/`Debug`，用 derive，无需独立生成文件 |
| `internal/diagnostics/generate.go`（`//go:build ignore` 代码生成器） | `internal/diagnostics/generate.rs`（或 `build.rs` / xtask） | 移植代码生成器本身：读 TS 子模块 JSON + LCL，产出上面两个 `_generated.rs` + `loc/*.json.gz`。见"代码生成"小节 |
| `internal/diagnostics/extraDiagnosticMessages.json` | 原样保留（生成器输入） | CLI/LS 专属额外消息（30 条，code 多为 100000+） |
| `internal/diagnostics/loc/*.json.gz`（14 个） | 原样保留（`include_bytes!` 输入） | 各语言译文 gzip+JSON，由生成器写出 |

## 依赖白名单（本包新增的 crate）

- `flate2`（gzip 解压 `loc/*.json.gz`，对应 Go `compress/gzip`）。
- `serde` + `serde_json`（反序列化译文 `map[Key]string` 与 `extraDiagnosticMessages.json`；对应 `internal/json`，与 Phase 1 `tsgo_json` 决策一致——库内用 `tsgo_json` 封装即可，不直接暴露 serde）。
- `regex`（占位符 `{(\d+)}`；或手写扫描省去依赖——**倾向手写**，见 TODO）。
- locale 匹配候选 `oxilangtag` / `icu_locid`（**P2 暂不引入**，用精简匹配；记 `// TODO(port)`）。
- 全部记入 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `diagnostics.rs`（Go: `internal/diagnostics/diagnostics.go`）

- [x] `#[repr(i32)] pub enum Category { Warning, Error, Suggestion, Message }`　`// Go: diagnostics.go:Category`
- [x] `impl Category { pub fn name(&self) -> &'static str }` — `warning/error/suggestion/message`　`// Go: diagnostics.go:Category.Name`（外加 `impl Display` 对应 stringer）
- [x] `pub type Key = &'static str`　`// Go: diagnostics.go:Key`
- [x] `pub struct Message { code: i32, category: Category, key: Key, text: &'static str, reports_unnecessary: bool, elided_in_compatibility_pyramid: bool, reports_deprecated: bool }`　`// Go: diagnostics.go:Message`
- [x] getters：`code()/category()/key()/reports_unnecessary()/elided_in_compatibility_pyramid()/reports_deprecated()`　`// Go: diagnostics.go:Message.Code 等`
- [x] `impl fmt::Display for Message`（返回 `text`，调试用）　`// Go: diagnostics.go:Message.String`
- [x] `impl Message { pub fn localize(&'static self, locale: &Locale, args: &[&str]) -> String }` — 委托 `localize(locale, Some(self), "", args)`　`// Go: diagnostics.go:Message.Localize`
- [x] `pub fn localize(locale: &Locale, message: Option<&'static Message>, key: &str, args: &[&str]) -> String` — message 为 None 时 `key_to_message(key)`；仍为 None 则 `panic("Unknown diagnostic message: ...")`；取译文（命中 locale 表则覆盖 text）→ `format(text, args)`　`// Go: diagnostics.go:Localize`
- [x] `fn get_localized_messages(locale: &Locale) -> Option<&'static FxHashMap<String,String>>` — `und` 直接 None；matcher 匹配 index→`LOCALE_FUNCS[index]`（懒加载）　`// Go: diagnostics.go:getLocalizedMessages`
- [x] `pub fn format(text: &str, args: &[&str]) -> String` — 无 args 直接返回；替换 `{n}`（手写扫描，无 regex 依赖），越界/解析失败 `panic("Invalid formatting placeholder")`　`// Go: diagnostics.go:Format`（`to_valid_utf8` 省略：Rust `&str` 恒为合法 UTF-8）
- [x] `pub fn stringify_args(args: &[Arg]) -> Vec<String>` — `Arg::Str` 原样，`Arg::Int` 用 `Display`（对应 Go `fmt.Sprintf("%v")`）；空返回空　`// Go: diagnostics.go:StringifyArgs`
- [x] ~~locale 缓存静态~~ **偏离**：未加独立 `Tag→messages` 缓存——每语言 `OnceLock` 已记忆解压，精简 matcher 为 O(tags)，Go 的 `sync.Map` 仅为省 matcher 成本，故省略

### `diagnostics_generated.rs`（Go: `internal/diagnostics/diagnostics_generated.go`，生成）

- [x] 2153 个 `#[doc(hidden)] pub static <NAME>: Message = Message { .. };`（由一次性脚本产出；`<NAME>` = Go 变量名转 `SCREAMING_SNAKE_CASE`，`key`/`text` 逐字节同 Go）　`// Go: diagnostics_generated.go`
- [x] `pub(crate) fn key_to_message(key: &str) -> Option<&'static Message>`（生成；`match` 2153 臂）　`// Go: diagnostics_generated.go:keyToMessage`

### `loc/mod.rs`（Go: `internal/diagnostics/loc_generated.go`）

> **偏离**：按本轮任务指示落地为 `mod loc;`（`loc/mod.rs`，与 13 个 `*.json.gz` 同目录，`include_bytes!` 路径更简），而非 crate 根的 `loc_generated.rs`。

- [x] `static MATCHER_TAGS: &[&str] = &["en", "zh-CN", "zh-TW", "cs-CZ", "de-DE", "es-ES", "fr-FR", "it-IT", "ja-JP", "ko-KR", "pl-PL", "pt-BR", "ru-RU", "tr-TR"]`（en 在 index 0）　`// Go: loc_generated.go:matcher`
- [x] `static LOCALE_FUNCS: &[Option<fn() -> &'static LocaleMessages>]`（index 0 = None/英语；其余懒加载函数）　`// Go: loc_generated.go:localeFuncs`
- [x] `fn load_locale_data(data: &[u8]) -> LocaleMessages`（=`FxHashMap<String,String>`）— flate2 解压 + `tsgo_json` 反序列化；失败 `panic`　`// Go: loc_generated.go:loadLocaleData`
- [x] 13 个 `static XX_DATA: &[u8] = include_bytes!("xx.json.gz");` + `static XX: OnceLock<LocaleMessages>` + 懒加载 fn　`// Go: loc_generated.go:zhCN 等`

### `generate.rs`（Go: `internal/diagnostics/generate.go`，代码生成器）

> **本轮状态（偏离）**：按任务指示，`diagnostics_generated.rs` 由**一次性脚本**（读现成的 Go `diagnostics_generated.go`，做 Go→Rust 字面量转写）产出，脚本**不入库**。完整移植 `generate.go`（读 TS 子模块 JSON/LCL、再生 `*.json.gz`）推迟——本机无 `_submodules/TypeScript`，且 `loc/*.json.gz` 现成可直接 `include_bytes!`。下列项保留未勾，待真正需要再生译文时移植。
>
> 代码生成器自身也要移植（执行期作 `xtask` 或 `build.rs`，**不**编入 `tsgo_diagnostics` 库）。逻辑 1:1：

- [ ] 读 `_submodules/TypeScript/src/compiler/diagnosticMessages.json` + 本包 `extraDiagnosticMessages.json`，按 code 合并（extra 覆盖）、按 code 升序排序　`// Go: generate.go:main/readRawMessages`
- [ ] `convert_property_name(orig, code) -> (var_name, key)`：`*`→`_Asterisk`、`/`→`_Slash`、`:`→`_Colon`、其余非字母数字→`_`；合并连续 `_`；去首/尾 `_`（首 `_` 后跟数字保留）；key=截断 100 + `_<code>`；非导出标识符前缀 `X`/`X_`　`// Go: generate.go:convertPropertyName`
- [ ] `generate_diagnostics` → `diagnostics_generated.rs`（含 `key_to_message`）　`// Go: generate.go:generateDiagnostics`
- [ ] `generate_localizations`：扫 `src/loc/lcl/*/diagnosticMessages/...lcl`（XML），按 known keys 过滤，按 key 排序后 JSON + gzip(BestCompression) 写 `loc/<tgtCul>.json.gz`，生成 `loc_generated.rs`　`// Go: generate.go:generateLocalizations/readLocalizedMessages`
- [ ] 注意 Rust 命名转换：生成的标识符建议用 `SCREAMING_SNAKE_CASE`（Rust static 约定），但 **`Key` 字符串须与 Go 完全一致**（key 是 locale 表与 `key_to_message` 的连接键，不可改）

### Cargo / crate 接线

- [x] `internal/diagnostics/Cargo.toml`（`name = "tsgo_diagnostics"`，deps：`tsgo_locale` `tsgo_json` `flate2` `rustc-hash`；`serde/serde_json` 经 `tsgo_json` 间接用；未用到的 `tsgo_core/tsgo_collections/tsgo_repo` 已移除）
- [x] 根 `Cargo.toml` workspace members（本 crate 已是 member，未改根 Cargo.toml）
- [x] `lib.rs`（即 `diagnostics.rs`）声明 `mod diagnostics_generated; mod loc;` + `pub use diagnostics_generated::*;`

## TDD 推进顺序（tracer bullet → 增量）

1. **手写一小撮 Message 常量**（`IDENTIFIER_EXPECTED`/`_0_EXPECTED`/`THE_PARSER_EXPECTED_...`）+ `format` + `Message::localize` 英语路径 → 先让 `TestLocalize` 的英语/Und/单参/多参 4 个 case 跑绿（tracer bullet，不依赖生成器与 locale 数据）。
2. **`format` 边界**（越界 panic、`ToValidUTF8`、无 args 短路）。
3. **精简 locale 匹配器** + 解压一种语言（如 `de-DE`）→ 跑绿德语 case，再补 fr/es/ja/zh/ko/ru 与 `af-ZA` 回落。
4. **`key_to_message`** → 跑绿 `TestLocalize_ByKey` 2 个 case。
5. **生成器** `generate.rs` 全量产出 2153 条 + 14 语言，替换手写常量，全量回归。

## 与 Go 的已知偏离（divergence）

- **locale 匹配**：Go 用 CLDR 语言距离（`x/text/language`）；Rust P2 用精简前缀匹配，覆盖单测全部 case，完整匹配 `// TODO(port)` 推迟（候选 `icu_locid`/`oxilangtag`）。端到端由 P10 兜底。
- **`StringifyArgs` 的 `%v`**：Go `fmt.Sprintf("%v", arg)` 对任意类型；Rust 侧诊断参数实际只有 `String`/数字/少量类型，用 `Display`/`enum Arg` 表达，非 `Box<dyn Any>`。须在调用点统一 arg 类型。
- **生成标识符大小写**：Go 用 `Identifier_expected`（导出驼峰）；Rust static 用 `IDENTIFIER_EXPECTED`。变量名是各 crate 内部引用、可改；但 **`key` 字符串与 `text` 必须逐字节一致**（参与本地化查表与最终文案）。
- **`Message` 单例**：Go 是堆上 `*Message`；Rust 是 `static Message`（`&'static`），引用更廉价、无分配——结构等价的改进。
- **`panic` 行为**：`Category.Name` 的未知分支、`Localize` 未知 key、`Format` 非法占位符均保留 panic 语义（`panic!`/`unreachable!`），与 Go 一致以便 conformance 对拍。

## 转交 / 推迟（DEFER）

- 完整 CLDR locale 匹配：`// TODO(port)`，blocked-by 选定 locale crate；P10 之前以精简匹配 + 英语回落工作。
- `ast.Diagnostic`（引用本包 `Message/Category/Key`）在 **ast/impl.md** 的 `diagnostic.rs` 落地，不属本 crate。
- 译文 `loc/*.json.gz` 的再生成依赖 TypeScript 子模块的 `src/loc/lcl`；若子模块缺失，生成器跳过 locale（保留已有 `.json.gz`），与 Go 行为一致。
