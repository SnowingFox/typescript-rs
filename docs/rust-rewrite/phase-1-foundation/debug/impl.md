# debug: 实现方案（impl.md）

**crate**：`tsgo_debug`　**目标**：编译器内部断言/失败原语（`Fail` / `Assert` / `AssertNever` / `FailBadSyntaxKind`），统一以 panic 形式上报"不应发生"的状态。
**依赖（crate）**：无（仅标准库 `fmt`）。叶子包。
**Go 源**：`internal/debug/`（1 个非测试文件：`debug.go` 62 行）

## 这个包是什么（业务说明）

`debug` 对应 strada（TS 原版）里的 `Debug` 命名空间，是 checker/binder/parser 等核心模块表达"内部不变量被违反"的统一出口。它故意以 panic（Go 的 `panic`）实现，因为这些断言失败代表编译器 bug 而非用户输入错误。

四个能力：
- `Fail(reason)`：无条件失败，消息前缀 `"Debug failure. "`。
- `FailBadSyntaxKind(node, msg...)`：遇到不该出现的 AST 节点种类时失败，附 `node.KindString()`。
- `AssertNever(member, msg...)`：穷尽性检查兜底（对应 TS 的 `assertNever`），打印值的 `KindString()`/`String()`/`%v`。
- `Assert(value, msg...)`：条件断言，false 时失败。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `panic(reason)` | `panic!("{reason}")` | Go panic → Rust panic；测试侧用 `#[should_panic]` 或 catch 断言消息（见 tests.md） |
| `message ...any`（可变参） | 形式上对应 `Option<&str>` 或 `fmt::Arguments` | Go 用 `fmt.Sprint(message...)` 拼接；Rust 侧建议用宏 `debug_fail!(...)` 或接受 `Option<String>`，对齐"无参=默认消息" |
| `node interface{ KindString() string }` | `trait KindString { fn kind_string(&self) -> String }` | `FailBadSyntaxKind` 接受实现该 trait 的类型 |
| `member any` + 类型断言 | `enum`/trait 对象 + `match` | `AssertNever` 的多态打印：优先 `KindString`，否则 `Display`（`fmt::Stringer`），否则 `{:?}`/`Debug` |
| `debug.Assert` | 本包 `assert!`/自定义 `assert` + `debug_assert!` 取舍 | PORTING §4：`debug.Assert` → 自定义 `assert`（始终启用，因 Go 侧无条件检查） |

> **关键决策**：Go 的 `debug.Assert` 是**始终生效**（非 `-tags` 受控），所以 Rust 应实现为始终检查的函数/宏，**不能**简单映射成只在 debug build 生效的 `debug_assert!`。消息格式必须逐字对齐（见测试期望值）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/debug/debug.go` | `internal/debug/lib.rs`（basename `debug` == crate 目录名 → `lib.rs`） | 全部断言原语 |

## 依赖白名单（本包新增的 crate）

- 无。仅标准库。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/debug/debug.go`）

- [ ] `pub fn fail(reason: &str) -> !` — 空 reason → `"Debug failure."`；否则 `"Debug failure. " + reason`；`panic!`（返回 `!`）　`// Go: debug.go:Fail`
- [ ] `pub trait KindString { fn kind_string(&self) -> String; }` — 供 `fail_bad_syntax_kind` / `assert_never` 使用　`// Go: debug.go`（接口 `interface{ KindString() string }`）
- [ ] `pub fn fail_bad_syntax_kind(node: &impl KindString, message: Option<&str>) -> !` — 默认消息 `"Unexpected node."`；格式 `"{msg}\nNode {kind} was unexpected."`　`// Go: debug.go:FailBadSyntaxKind`
- [ ] `pub fn assert_never<T>(member: T, message: Option<&str>) -> !` — 默认 `"Illegal value:"`；detail 优先 `KindString` → `Display` → `Debug`；格式 `"{msg} {detail}"`　`// Go: debug.go:AssertNever`
- [ ] `pub fn assert(value: bool, message: Option<&str>)` — false 时走 `assert_slow`　`// Go: debug.go:Assert`
- [ ] `fn assert_slow(message: Option<&str>) -> !` — 有消息 `"False expression: " + msg`，否则 `"False expression."`，再 `fail`　`// Go: debug.go:assertSlow`

> 注：Go 的可变参 `message ...any` 在 Rust 可用宏包装（`debug_fail!`, `debug_assert_msg!`）以支持任意格式化；但**默认消息与拼接结果必须逐字对齐**测试期望。建议宏 + 底层 `fn` 双轨。

### Cargo / crate 接线

- [ ] `internal/debug/Cargo.toml`（`name = "tsgo_debug"`）
- [ ] 根 `Cargo.toml` workspace members 追加本 crate
- [ ] `lib.rs` 公开 re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `fail` + 消息前缀逻辑（最底层，所有其它函数依赖它）；2 个子用例（空/非空 reason）做 red→green。
2. `assert` / `assert_slow`（true 不 panic、false panic + 消息）。
3. `KindString` trait + `fail_bad_syntax_kind`（mock 节点）。
4. `assert_never` 的三路 detail 选择（KindString / Display / Debug 兜底）。

## 与 Go 的已知偏离（divergence）

- **panic 消息捕获**：Go 测试用 `testutil.AssertPanics(t, fn, expectedMsg)` 捕获 panic 值并比对字符串。Rust 侧用 `std::panic::catch_unwind` + 向下转型为 `&str`/`String` 比对，或用 `#[should_panic(expected = "...")]`。需保证 panic payload 为可比对的字符串。
- **`message ...any` 拼接**：Go `fmt.Sprint(message...)` 对多参数无分隔符直连。Rust 宏需复现这一拼接语义（无空格分隔，除非相邻都非字符串——Go `Sprint` 规则）。本包测试只用单字符串参，先按单参对齐，多参语义标 `// TODO(port)`。
- **`runtime.Breakpoint()`**：Go 里被注释掉，Rust 无需对应。

## 转交 / 推迟（DEFER）

- `KindString` trait 的真实实现者（AST 节点）在 P2/P3 才出现；本包只定义 trait + 用 mock 节点测试。
