# locale: 实现方案（impl.md）

**crate**：`tsgo_locale`　**目标**：表示并在 `context.Context` 中传递界面语言（locale），用于诊断消息本地化。
**依赖（crate）**：`unic-langid`（或等价 BCP-47 语言标签解析）。叶子包。
**Go 源**：`internal/locale/`（1 个非测试文件：`locale.go` 29 行）

## 这个包是什么（业务说明）

`locale` 把 `--locale` 选项解析成一个语言标签，并通过 `context.Context` 把它带到诊断格式化层（让 checker/diagnostics 输出对应语言的消息）。逻辑极薄：

- `Locale` = `language.Tag` 的别名（来自 `golang.org/x/text/language`）。
- `Default` = 零值 Locale。
- `WithLocale(ctx, locale)` / `FromContext(ctx)`：在 context 里存/取（用私有 `contextKey(0)`）。
- `Parse(localeStr)` → `(Locale, ok bool)`：宽松解析，失败返回 `ok=false`（不报错）。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `language.Tag`（x/text） | `unic_langid::LanguageIdentifier`（或自定义 newtype） | BCP-47 标签；Rust 用 `unic-langid` 解析 |
| `type Locale language.Tag` | `pub struct Locale(LanguageIdentifier)` 或 `type Locale = LanguageIdentifier` | newtype 包装更安全 |
| `var Default Locale`（零值） | `Locale::default()` / `const`/`OnceLock` | 零值标签 |
| `context.Context` + `contextKey` | 显式传参 `&Locale` 或 `Arc<Locale>`（PORTING §3：不引入 async context） | **关键偏离**：Go 用 `context.Value` 隐式携带；Rust 应显式把 `Locale` 作为参数/字段下传到 diagnostics |
| `Parse → (Locale, ok)` | `fn parse(s: &str) -> Option<Locale>` | 宽松解析，失败 `None`（对齐 `ok=false`） |

> **核心偏离**：`context.Context` 在 Rust 无直接对应，且 PORTING §3 明确"`context.Context` → 显式传 `&Cancel`/`Arc<...>`"。本包的 `WithLocale`/`FromContext` 在 Rust 不再用隐式 context，而是把 `Locale` 作为显式参数注入需要它的诊断 API。这条偏离须在 P2（diagnostics）落地时贯彻，本包先提供 `Locale` 类型 + `parse`。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/locale/locale.go` | `internal/locale/lib.rs`（basename == crate 目录名 → `lib.rs`） | Locale 类型 + parse |

## 依赖白名单（本包新增的 crate）

- `unic-langid`（BCP-47 语言标签解析；对齐 `golang.org/x/text/language.Parse` 的宽松解析）。记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/locale/locale.go`）

- [x] `pub struct Locale(LanguageIdentifier)`（或别名） + `Default`　`// Go: locale.go:Locale/Default`
- [x] `pub fn parse(locale_str: &str) -> Option<Locale>` — 宽松解析，失败返回 `None`（对齐 Go `(Locale, ok)` 的 `ok=false`）　`// Go: locale.go:Parse`
- [x] （偏离说明）原 `WithLocale`/`FromContext` 改为显式传参，不在本 crate 提供 context 注入；如需轻量传递，提供 `Locale` 的 `Clone`/`Copy`　`// Go: locale.go:WithLocale/FromContext`（DEFER 到 diagnostics 接线）

### Cargo / crate 接线

- [x] `internal/locale/Cargo.toml`（`name = "tsgo_locale"`，dep `unic-langid`）
- [x] 根 `Cargo.toml` workspace members 追加
- [x] `lib.rs` re-export `Locale` / `parse`

## TDD 推进顺序（tracer bullet → 增量）

1. `Locale` 类型 + `parse` 合法标签（`"en"`/`"zh-CN"`/`"ja"`）—— tracer bullet。
2. `parse` 非法标签返回 `None`（对齐宽松失败）。
3. `Default` 语义（零值标签）。

## 与 Go 的已知偏离（divergence）

- **context 携带 → 显式传参**：见上文核心偏离。`WithLocale`/`FromContext` 不在本 crate 复刻，改由 diagnostics API 接收 `Locale` 参数。
- **语言标签库替换**：`golang.org/x/text/language` → `unic-langid`。解析宽松度需对齐（合法 → Some，非法 → None）；边界标签（私有用途子标签等）由 tests/P10 校验。

## 转交 / 推迟（DEFER）

- locale 的实际"携带 + 应用到诊断消息"在 P2（diagnostics）落地：`// DEFER(phase-2) blocked-by: diagnostics`。
