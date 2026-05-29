# <pkg>: 实现方案（impl.md 模板）

> 复制本模板到 `phase-N-*/<pkg>/impl.md`。删除本行与所有 `<...>` 占位说明。
> 写之前**必须实际读** `internal/<pkg>/*.go`（非测试）逐个文件。所有 TODO 带 `// Go:` 锚点。
> 语言：本规划文档用中文；但**生成的 `.rs` 代码里所有注释必须英文**（rustdoc `///`/`//!`、行内 `//`、`Side effects`、marker 等），见 [PORTING.md §7](../PORTING.md)。
> TDD：实现期**绝对遵循 `/tdd` SKILL**（红→绿逐行为，禁止"先翻完实现再补测试"），见 [references/tdd.md](./tdd.md)。

**crate**：`tsgo_<pkg>`　**目标**：<一句话：这个包干什么>
**依赖（crate）**：`tsgo_xxx` `tsgo_yyy`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/<pkg>/`（<N> 个非测试文件，<LOC> 行）

## 这个包是什么（业务说明）

<2-4 段：在编译器/语言服务里的角色、上下游、为什么在这个 phase。>

## 所有权 / 类型映射（本包关键决策）

<只写本包特有的映射决策。通用规则见 PORTING.md §3/§5。>

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `<GoType>` | `<RustType>` | <为何这样映射 / 偏离 Go 语法处> |

<若涉及 arena/索引/并发，单独一小节说明所有权图。>

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/<pkg>/<file>.go` | `internal/<pkg>/<file>.rs`（或 lib.rs/mod.rs） | <核心内容> |

## 依赖白名单（本包新增的 crate）

<本包要用到、PORTING §10 之外的 crate；执行期 `cargo add`，记到 references/crate-map.md。>

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按"包内依赖序 / TDD 推进序"。每条 `[ ]`，实现后改 `[x]`。

### `<file>.rs`（Go: `internal/<pkg>/<file>.go`）

- [ ] `pub fn <name>(...) -> ...` — <行为>　`// Go: <file>.go:<Func>`
- [ ] `pub struct <Name> { ... }` — <字段/不变量>
- [ ] ...

### Cargo / crate 接线

- [ ] `internal/<pkg>/Cargo.toml`（`name = "tsgo_<pkg>"` + path deps）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] `lib.rs` 声明子模块 `mod <file>;` + 公开 re-export

## TDD 推进顺序（tracer bullet → 增量）

1. <第一个最小可验证的函数/类型 + 它对应的测试>
2. <下一个，依据上一步学到的>
3. ...

## 与 Go 的已知偏离（divergence）

<如 `node.Parent` → `node.parent(arena)`、并行点先顺序化等，逐条记录 + 原因。>

## 转交 / 推迟（DEFER）

<本包暂时实现不了、依赖后续 phase 的项；标 `// DEFER(phase-N) / blocked-by:`，并在此列出。>
