# PORTING.md — typescript-go → Rust 移植契约

> 本文件是所有 phase / 所有 subagent 写代码、写文档时的**共享契约**。动手前必读。
> 灵感来自 Bun 的 Zig→Rust `PORTING.md`（结构优先、逐文件 1:1），但我们**不**照搬其
> "不需要编译 / 禁用 rayon" 的约束。我们要的是：**测试先行的逐文件 1:1 移植 + 真编译 + 真并发**。
>
> 🔴 **头号铁律：绝对遵循 `/tdd` SKILL（红→绿垂直切片）。** 先有红的测试，才允许写让它变绿的实现。
> **移植不豁免 TDD**：即使 Go 的 impl+test 都现成，也禁止"先把实现整文件翻完再补测试"（横切反模式）。
> 完整规则 + `/tdd` 原文见 **[references/tdd.md](./references/tdd.md)（写任何 `.rs` 前必读）**。

## 0. 一句话目标

把 `github.com/microsoft/typescript-go`（Go 1.26，~30.4 万行实现 + 4383 个测试文件）
逐文件 1:1 移植到 **安全 Rust**（尽量零 `unsafe`），源 `.go` 不删，`.rs` 紧贴同目录同名，
每个模块**先移植测试（red）→ 再移植实现（green）**，编译通过 + 测试全绿才进入下一个。

## 1. 已敲定的方法论决策（grill 结论，不可推翻）


| #   | 决策       | 结论                                                                                                                                                                   |
| --- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 方法论      | **严格 TDD（`/tdd` SKILL，红→绿垂直切片）**：**逐行为**地"移植一个 Go 测试→看到它红→移植该函数实现→变绿"，一次一个，绝不横切（禁止先翻完整文件实现再补测试）。模块必须真编译 + 测试全绿才进入下一个。完整规则见 [references/tdd.md](./references/tdd.md)。 |
| 2   | 代码位置     | **同仓库 side-by-side**：`internal/core/core.go` 旁放 `internal/core/core.rs`，不删 Go 源。                                                                                     |
| 3   | crate 布局 | 仓库根一个 **Cargo workspace**；每个 `internal/<pkg>` = 一个 crate，名 `tsgo_<pkg>`。                                                                                             |
| 4   | 分阶段      | 按真实依赖 DAG 的 **10 个 phase**（见 README）。每个包/子阶段一个目录，含 `impl.md` + `tests.md`。                                                                                           |
| 5   | 注释       | 所有公开项必须有 rustdoc `///`，含 `# Examples`（input/output）与 **Side effects** 说明。                                                                                            |
| 5b  | 单测覆盖     | **每个函数都要有单测**（即使 Go 没测）；Go 现有 `*_test.go` 全量移植一条不少。详见 §8.5/§8.6。                                                                                                     |
| 5c  | 超大文件     | Go 巨无霸文件（如 `checker.go`）拆成子目录多文件（如 `checker/core/`），各带单测。详见 §2「超大文件拆分规则」。                                                                                            |
| 6   | 测试范围     | 早期按包把 `*_test.go` 单测 1:1 移植作 gate；`fourslash`(4250) + `testdata`(294MB) 推迟到 **P10** 做端到端 parity。                                                                     |
| 7   | AST 所有权  | **arena + 类型化索引（`NodeId`）**；零 `unsafe`；`Parent`/子节点都用索引。                                                                                                             |
| 8   | 并发       | **真并发**（要性能收益）：数据并行用 `rayon`，生产者/消费者用 `std::thread` scoped + channel；输出保持确定性以便断言。CPU-bound，不引入 tokio。                                                                |


## 2. 仓库与 crate 布局

```
typescript-go/
├── Cargo.toml                 # [workspace] members = ["internal/stringutil", ...]（随 phase 增量追加）
├── rust-toolchain.toml        # 钉工具链版本，edition = 2021
├── internal/
│   ├── stringutil/
│   │   ├── util.go            # 原 Go（不删）
│   │   ├── util.rs            # 移植产物
│   │   ├── util_test.go       # 原 Go 测试（不删）
│   │   ├── Cargo.toml         # name = "tsgo_stringutil"
│   │   └── lib.rs             # crate 根（见命名规则）
│   ├── core/ ...
│   └── ...
└── docs/rust-rewrite/         # 本套文档
```

### 文件命名规则（照 Bun）

- `.rs` 与 `.go` **同目录、同 basename**。
- 若 basename == crate 根目录名 → `lib.rs`（如 `internal/core/core.go` → `internal/core/lib.rs`，且 crate `tsgo_core` 的入口）。
- 若 basename == 其直接父目录名（嵌套子模块）→ `mod.rs`。
- 其余 → 同名 `.rs`（`internal/core/compare.go` → `internal/core/compare.rs`，在 `lib.rs` 里 `mod compare;`）。
- 嵌套子包（如 `internal/ls/lsconv/`、`internal/vfs/iovfs/`）作为父 crate 的子 module 或独立子 crate，由所属 phase 的文档决定，但**默认作父 crate 的子 module**，除非有独立外部依赖。

### 测试文件命名（**单测独立成文件，镜像 Go `_test.go`**）

- **不要把测试内联进实现文件**（禁止在 `<file>.rs` 里写 `#[cfg(test)] mod tests { ... }` 大块）。
- 每个有测试的 `<file>.rs` 旁配一个兄弟文件 `**<file>_test.rs`**（对应 Go 的 `<file>_test.go`，如 `util.rs` ↔ `util_test.rs`、`lib.rs` ↔ `lib_test.rs`）。
- 在 `<file>.rs` **末尾**用一行挂载（保持同模块、可访问私有项）：

```rust
#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
```

- `<file>_test.rs` 以 `use super::*;` 开头，即可像 Go 同包测试一样访问 `<file>.rs` 的私有项。
- `#[path]` 相对当前文件目录解析，故测试文件与实现文件**同目录并排**，与 `.go`/`_test.go` 布局一致。

### 超大文件拆分规则（mega-file decomposition）

Go 里有若干"巨无霸"单文件（最典型 `internal/checker/checker.go` 数万行；也包括 `internal/checker/*.go` 的几个大文件、`ls`/`parser` 的大文件）。**1:1 同名映射对这种文件不适用**——一个几万行的 `.rs` 既不可维护、也无法 TDD、还会拖垮编译。规则：

- **阈值**：单个 Go 文件 **> ~1500 行**（或语义上明显多职责）时，**拆成一个子目录模块**，按职责分多个内聚 `.rs`，每个 `.rs` 配兄弟 `<stem>_test.rs`。
- **目录约定**：在该包下开一个子目录承载拆分件。`checker.go` → `**internal/checker/core/`** 子目录（用户指定），里面按子系统分文件，例如：
`core/mod.rs`（`Checker` 结构 + 入口）、`core/relations.rs`（关系/子类型）、`core/inference.rs`（推断）、`core/instantiation.rs`（实例化）、`core/flow.rs`（控制流分析）、`core/types.rs`（类型构造）、`core/symbols.rs`（符号解析）… 每个都带 `*_test.rs`。
- **映射可追溯**：拆分后，在该包 `impl.md` 写一张"Go 源文件 → Rust 子文件"对照表，每个 Rust 函数仍带 `// Go: internal/<pkg>/<file>.go:<Func>` 锚点（锚到**原** Go 文件/函数，不因拆分而丢失来源）。
- **每函数单测照旧**（见 §8.6）：拆出来的每个文件里的函数都要有单测。
- 这是**受认可的、必要的对 1:1 文件映射的偏离**，须在该包 `impl.md` 顶部"拆分说明"小节记录（哪个大文件拆成了哪些子文件、为什么）。
- 同理适用于超大**测试**：一个 Go `xxx_test.go` 若过大，可拆成 `<sub>_test.rs` 多个，按被测子文件就近放置。

### crate 命名

- crate 名一律 `tsgo_<pkg>`（snake_case）：`tsgo_core` / `tsgo_ast` / `tsgo_checker` …
- crate 间依赖在各自 `Cargo.toml` 的 `[dependencies]` 用 `path` 声明，镜像 Go 的 import 边。

## 3. Go → Rust 类型映射（标准表，写进每个 impl.md 的"类型映射"小节）


| Go                            | Rust                                                     | 备注                                                                        |
| ----------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------- |
| `string`                      | `String` / `&str`                                        | 拥有用 `String`，借用用 `&str`；TS 源码字符串是 UTF-8                                   |
| `[]byte`                      | `Vec<u8>` / `&[u8]`                                      |                                                                           |
| `[]T`                         | `Vec<T>` / `&[T]`                                        |                                                                           |
| `map[K]V`（无序）                 | `FxHashMap<K, V>`（`rustc_hash`）                          | 默认；需确定性输出时见下                                                              |
| `map[K]V`（需保插入序 / 决定输出）       | `indexmap::IndexMap<K, V>`                               | **凡影响 emit/诊断顺序的 map 必须用 IndexMap**                                       |
| Go `set`（`map[T]struct{}`）    | `FxHashSet<T>` / `IndexSet<T>`                           | 同上规则                                                                      |
| `int` / `int32` / `int64`     | `i32` / `i64`（按语义）                                       | TS 源里多为 `int`→`i32`；位置/偏移用 `i32`（对齐 Go 的 `int` 截断行为，存疑处加 `// PERF(port)`） |
| `float64`                     | `f64`                                                    | `jsnum` 包专门处理 JS number 语义                                                |
| `bool`                        | `bool`                                                   |                                                                           |
| `rune`                        | `char`                                                   | 注意 Go rune 是 i32 code point；UTF-16 处理见 scanner                            |
| `*T`（可空指针）                    | `Option<Box<T>>` / `Option<Idx>`                         | AST 节点一律用 `NodeId` 索引（见 §5）                                               |
| `*T`（非空、共享）                   | `Rc<T>` / arena 索引                                       | 优先 arena 索引；symbol/type 等长生命周期对象用 arena                                   |
| `interface{ ... }`（行为）        | `trait` + `dyn`/泛型                                       |                                                                           |
| `interface{}` / `any`         | `enum` (判别联合) 优先；不得已用 `Box<dyn Any>`                     | AST `nodeData` → `enum NodeData`                                          |
| Go `iota` 枚举                  | `#[repr(i32)] enum` 或 `bitflags!`                        | flags 类（`NodeFlags`/`SymbolFlags`/`ModifierFlags`）用 `bitflags` crate      |
| `error` / `(T, error)`        | `Result<T, E>`                                           | 见 §4 错误处理                                                                 |
| `nil`（接口/指针）                  | `None`                                                   |                                                                           |
| `sync.Mutex` / `sync.RWMutex` | `Mutex` / `RwLock`（parking_lot 可选）                       |                                                                           |
| `sync.Map`                    | `dashmap::DashMap` 或 `Mutex<HashMap>`                    |                                                                           |
| `atomic.Uint64`               | `AtomicU64`                                              |                                                                           |
| goroutine + channel           | `std::thread::scope` + `crossbeam-channel`；数据并行用 `rayon` | 见 §6                                                                      |
| `context.Context`             | 显式传 `&Cancel`/`Arc<AtomicBool>` 取消标志                     | 不引入 async                                                                 |


## 4. Go → Rust 惯用法映射

- **错误处理**：Go `(T, error)` → `Result<T, E>`，`if err != nil { return ... }` → `?`。
panic 性质的 `panic()` → Rust `panic!`/`unreachable!`；`debug.Assert` → `debug_assert!`/自定义 `assert`。
- **多返回值** → 元组 `(A, B)` 或具名 struct。
- **零值** → `Default::default()`；注意 Go 零值语义（空 slice vs nil）在边界处用注释标注。
- `**defer`** → RAII（`Drop`）或 scope guard（`scopeguard` crate）。
- **方法 receiver** → `impl` 块；指针 receiver `(n *Node)` 改 `&mut self`，值 receiver `&self`。
- **嵌入（embedding）** → 组合 + 委托（无继承）；公共字段用组合 struct，方法手写转发或用 `Deref`（慎用）。
- **泛型** → Rust 泛型 + trait bound。Go 1.26 泛型基本可直译。
- **接口断言 `x.(T)`** → `match`/`if let` on enum，或 `Any::downcast`。
- `**switch x.(type)**` → `match` on enum 判别。
- **命名**：Go `CamelCase` 导出 → Rust `snake_case` 函数/字段、`CamelCase` 类型；缩写折叠成一个小写词（`toJSON`→`to_json`、`isCSS`→`is_css`、`getURL`→`get_url`）。

### Marker 注释（机检友好）

- `// PERF(port): <Go idiom> — 见 Phase B`：凡用了 Go 的性能特化写法（arena 预分配、`appendAssumeCapacity` 类）而当前用标准 Rust 等价物替代处。
- `// TODO(port): <原因>`：移植中暂时简化、待回填处。
- `// DEFER(phase-N): <原因> / blocked-by: <依赖>`：依赖尚未移植、推迟到 phase N 的占位。
- `// Go: internal/<pkg>/<file>.go:<func>`：每个 Rust 公开函数若有 Go 上游对应物，标注锚点（行号会漂移，用 `<file>:<func>` 锚）。

## 5. AST 所有权模型（命门：决定能否零 unsafe）

Go 的 AST 是 **arena + 裸指针图**：单一 `ast.Node{ Kind, Flags, Loc, Parent *Node, data nodeData }`，
`data` 是数百个实现的接口（相当于判别联合），节点由 `NodeFactory` 的各类型 arena 分配，
`NodeList{ Nodes []*Node }`，且有 `Parent` 反向指针 + 绑定期可变。

**Rust 表示（统一约定）：**

- 用一个 **arena**（如 `Vec<NodeData>` 或 `id-arena`/`la-arena` crate）持有所有节点。
- `NodeId` 是 newtype 索引（`#[derive(Copy)] struct NodeId(u32)`），**所有引用（含 `Parent`、子节点、`NodeList`）都用 `NodeId`**，不用 Rust 引用 `&`。
- `nodeData` 接口 → `enum NodeData { Identifier(...), CallExpression(...), ... }`，`Kind` 与之对应。
- 访问器从 `n.Parent` → `arena[id].parent` 或 `id.parent(&arena)`；调用点改成 arena/handle 访问器。
- 这样环、反向指针、绑定期可变都能用安全 Rust 表达（rust-analyzer / swc 的主流做法）。
- 同模式适用于 `Symbol` / `Type` 图（checker 的 `TypeId` / `SymbolId` arena）。

> 偏离字面 Go 语法（`node.Parent` → `node.parent(arena)`），但**结构 1:1 保留**。
> 这是允许的、且必要的偏离，须在该包 impl.md 顶部"所有权模型"小节写明。

## 6. 并发

- typescript-go 用 goroutine/channel 并行（~289 处 goroutine/sync、41 处 chan），主要在 checker / compiler / project。
- Rust 侧**按站点选最合适原语**：
  - 数据并行（map 一批文件/节点）→ `rayon` 的 `par_iter`。
  - 生产者-消费者 / worker 池 → `std::thread::scope` + `crossbeam-channel`。
- **输出必须确定性**：并行收集后按稳定 key 排序，保证诊断/emit 顺序与 Go 一致（这是 TDD 断言的前提）。
- 不引入 `tokio` 等 async 运行时（编译器是 CPU-bound）。
- 第一遍若并行点难直译，可先写顺序版 + `// PERF(port)`，并在该包 impl.md 标注待并行化；但**不强制**先单线程——能直接安全并行就并行。

## 7. rustdoc 注释规范（doc 质量红线）

> **语言铁律：所有代码里的注释一律用英文** —— rustdoc `///`/`//!`、行内 `//`、`# Examples`、`Side effects`、`// Go:`/`// PERF(port)`/`// TODO(port)`/`// DEFER`/`// SAFETY:` 等全部用英文。
> 规划类 markdown（`impl.md`/`tests.md`/各 `README.md`）仍用中文；**只有 `.rs` 源码内的注释必须英文**。
> 理由：这是 Microsoft 官方仓库的 Rust 移植，代码注释面向国际维护者，须与 Go 上游一致用英文。该红线由 gate-code 的 C8 机检（`.rs` 注释禁出现 CJK 字符）。

**每个公开项**（`pub fn` / `pub struct` / `pub enum` / `pub trait`）必须：

1. 多行 `///` summary（英文）：说清**意图**，不复读签名。
2. `# Examples`：给 input → output 的最小例子（doctest 能跑最好）。
3. **Side effects** 段：有 I/O（读写文件 / 调子进程 / 改全局）的函数必须列副作用清单 + "没动什么"（防误解，如 `// Worktree / index / HEAD: unchanged`）。纯函数注明 `Side effects: none (pure)`。
4. 不允许 section divider 注释（`// ==== xxx ====`）划分文件区块。
5. 不允许 `# Arguments` / `# Returns` 复读类型签名。

示例（注意：注释全英文）：

```rust
/// Percent-encodes a string per RFC3986 `encodeURI`, preserving the reserved
/// set (`;/?:@&=+$,#`).
///
/// Mirrors TS `encodeURI`: non-ASCII is emitted as `%XX` over its UTF-8 bytes.
///
/// # Examples
/// ```
/// assert_eq!(encode_uri("a b"), "a%20b");
/// assert_eq!(encode_uri(";/?:@&=+$,#"), ";/?:@&=+$,#");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:EncodeURI
pub fn encode_uri(input: &str) -> String { /* ... */ }
```

## 8. 测试对齐规范（单测 1:1 的硬要求）

这是本次移植的**核心纪律**——`tests.md` 必须与真实 Go 测试逐条对齐：

1. **测试放独立文件**：Rust 测试写在兄弟文件 `<file>_test.rs`（镜像 Go `<file>_test.go`），用 `#[cfg(test)] #[path="<file>_test.rs"] mod tests;` 挂在 `<file>.rs` 末尾，测试文件以 `use super::*;` 开头（见 §2）。**禁止内联大块 `#[cfg(test)] mod tests {}`**。
2. **逐 `func Test*` 对齐**：每个 Go `func TestXxx` 对应一个或多个 Rust `#[test]`。
3. **钻进表驱动子用例**：Go 测试多为 `tests := []struct{...}{...}` + `t.Run(tt.name, ...)`。
  `tests.md` 必须把**每个子用例**（`name` + input + expected）列成一行，不能只列顶层函数。
   Rust 侧对应用 `#[test]` 逐 case 或 `rstest` 参数化。
4. **ground truth 用 Go 实测值**：expected 值取自 Go 测试里的字面量，不得用 Rust 侧推断。
5. `**// Go:` 锚点**：每个 Rust 测试文件 / 用例标 `// Go: internal/<pkg>/<file>_test.go:<TestFunc>/<case-name>`。
6. **Go 现有单测一条都不能少**：所有 `*_test.go` 的 `func Test*` 及其表驱动子用例**必须全量移植**，不得挑选、不得省略。
7. **🔴 每个函数都必须有单测（即使 Go 没测）**：Go 的单测覆盖很稀疏，但我们的标准更高——**每个公开函数（`pub fn`）、以及行为不平凡的私有函数，都必须至少有一条 Rust 单测**，遵循 `/tdd` 红→绿写出（不是事后补）。
  - Go 有对应测试的函数：移植其全部子用例（见 5），expected 取 Go 字面量。
  - **Go 没有测试的函数**：自己写行为级单测——优先用 Go 实现的确定性语义 / TS spec 已知值做 expected；纯函数给 input→output，有副作用的给可观察行为。
  - 旧表述"0 直接单测的包只补少量 + 靠 P10 兜底"**作废**：P10 conformance/fourslash 仍是端到端兜底，但**不能替代每函数单测**。0-Go-test 的包（`scanner`/`evaluator`/`json`/`glob`/`nodebuilder`/`locale`/`pprof`/`repo` 等）同样要做到"每函数一测"。
  - 真正无法单测的函数（如纯 I/O 编排、需完整下游栈）才允许 `—` 推迟，并在 `tests.md` 注明 `blocked-by` 与目标 phase。
8. `tests.md` 的"完成"列：`✓`=Rust 已有对应用例且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase（须带 `blocked-by`）。
9. **收口自检**：包收口前，对照该 crate 的 `pub fn` 清单核对——每个都应能在 `<stem>_test.rs` 找到至少一条 `#[test]`（或 doctest）覆盖；缺的要么补测、要么显式 `—`+`blocked-by`。
10. **🔴 覆盖率硬指标：单测只能比 Go 多、不能比 Go 少，且每个 crate 的行覆盖率目标 ≥ 90%**。
  - **比 Go 多不能少**：Go `*_test.go` 的全部用例必须 1:1 移植（见 6），并在其之上**为每个重要函数额外补测**（公开 `pub fn` 必测；行为不平凡的私有函数也要测——即使 Go 源码完全没测）。宁可多测，确保稳定性。
  - **≥90% 行覆盖**：每个 crate 收口时行覆盖率应到 90% 以上。落地路线**仍是 `/tdd` 红→绿逐行为**（不是先堆代码再回填测试，不是横切）：每加/改一个行为，就先写一条会红的行为级测试。覆盖率是结果，不是借口写空测。
  - 测**可观察行为**（公开接口的 input→output / 副作用），不要为凑覆盖去测私有实现细节或写无断言的"形状测试"（违背 `/tdd` 哲学）。
  - 真正无法单测的函数（纯 I/O 编排 / 需完整下游栈）才允许 `—`+`blocked-by`；这类不计入"应覆盖"分母，但要在 `tests.md` 显式列出。
  - **每个 subagent 轮次都按此推进**：报告里给出该轮 `cargo test` 计数增量，体现"只增不减、向 90% 靠拢"。

## 9. 每个模块的 TDD 循环（执行期，文档先于代码）

文档阶段（本轮）只产出 `impl.md` + `tests.md`，但必须写成"可被直接执行 TDD 的脚本"：

1. `impl.md` 列出该包**全部非测试 `.go` 文件** → 对应 `.rs`，每个文件下列可勾选的函数级 TODO（带 `// Go:` 锚）。
2. `tests.md` 列出该包**全部 `*_test.go`** → 对应 Rust 测试，逐子用例对齐（见 §8）。
3. 二者必须**互相对齐**：impl.md 里每个要实现的公开函数，若 Go 有测试，tests.md 必须有对应行；反之 tests.md 每条用例，impl.md 必须有承载它的实现 TODO。

## 10. 依赖白名单（Go 依赖 → Rust crate 映射，写进 references/crate-map.md）


| 用途            | crate                                 |
| ------------- | ------------------------------------- |
| 有序 map/set    | `indexmap`                            |
| 快速 hash       | `rustc_hash`（FxHashMap）               |
| bitflags 枚举   | `bitflags`                            |
| arena         | `la-arena` / `id-arena`（择一，全仓统一）      |
| 数据并行          | `rayon`                               |
| channel       | `crossbeam-channel`                   |
| 并发 map        | `dashmap`                             |
| 序列化（LSP/JSON） | `serde` / `serde_json`                |
| 错误            | `thiserror`（库内）；不引入 `anyhow` 到库公开 API |
| 临时目录（测试）      | `tempfile`                            |
| 参数化测试         | `rstest`                              |


> 新增依赖必须用最新稳定版（执行期用 `cargo add`，不要瞎编版本号），并记到 `references/crate-map.md`。

## 11. 质量 Gate（把本契约变成可运行门禁）

本契约里的红线（rustdoc / 命名 / `// Go:` 锚 / 零 unsafe / checkbox 纪律 / DEFER+blocked-by / 依赖序）
都由 `docs/rust-rewrite/scripts/` 下的脚本**机检**，完整说明见 **[references/gates.md](./references/gates.md)**：

```bash
bash docs/rust-rewrite/scripts/gate-docs.sh   # 文档 gate（纯 bash+ripgrep，不依赖 Rust 代码）
bash docs/rust-rewrite/scripts/gate-code.sh   # 代码 gate（无 Cargo.toml 时优雅 no-op）
bash docs/rust-rewrite/scripts/gate.sh        # 聚合：--docs-only | --code-only | --strict
```


| Gate                        | 对应本文档红线                             |
| --------------------------- | ----------------------------------- |
| 文档 D2（`// Go:` 锚）           | §4 marker、§8 测试对齐                   |
| 文档 D3（checkbox 纪律）          | §9 可执行 TDD 脚本                       |
| 文档 D5（crate 名 / 无 divider）  | §2 命名、§7.4 禁 divider                |
| 文档 D6（依赖序倒置）                | §1 决策 4（按真实依赖 DAG 排 phase）          |
| 代码 C4（unsafe 须 SAFETY）      | §0/§5 尽量零 unsafe                    |
| 代码 C5（rustdoc missing_docs） | §7 rustdoc 红线                       |
| 代码 C6（test-go-parity）       | §8 测试 1:1 对齐                        |
| 代码 C7（stub-readiness）       | §4 `DEFER(phase-N)` + `blocked-by:` |


**纪律**：每个包/phase 收口前必须 `gate.sh` 全绿，才能在 README 进度打 `[x]`（镜像 story「README 顶层 `[x]` ≠ 真完成」）。