# checker: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 个测试文件 / 3 个 `func`（2 个 `Test*` + 1 个 `Benchmark`）。**这是 5.9 万行代码的全部包内单测**——checker 的正确性**几乎完全靠 P10 conformance/fourslash/`.d.ts` baseline 端到端对拍**。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/checker/checker_test.go` | `internal/checker/tests/checker.rs`（集成：用 `compiler`/`tsoptions`/`vfstest` 起 program） | `TestGetSymbolAtLocation`（1）+ `BenchmarkNewChecker`（→ P10 性能） |
| `internal/checker/tracer_test.go` | `internal/checker/tracer.rs`（`#[cfg(test)] mod tests`，内部包，需访问 `NewTracer`/`testTraceEvent`） | `TestTracerPushPreservesEndArgMutations`（1） |

## `checker_test.go`

### `TestGetSymbolAtLocation`（单块，端到端）

> 起一个内存 program（`vfstest` + `bundled.WrapFS` + `compiler.NewProgram`），bind 后取 checker，对 3 个节点（interface 名、变量名、属性访问）各 `GetSymbolAtLocation`，断言均非空。这是 checker 第一个 tracer bullet（子阶段 **4b** 收口判据）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `get_symbol_at_location` | 三类节点都能取到非空符号 | 源码 `interface Foo{bar:string} declare const foo:Foo; foo.bar;` + tsconfig(files:["foo.ts"])；bind 后对 `[interfaceId, varId, propAccess]` 各 `get_symbol_at_location(node)` → 均 `Some(symbol)` | `checker_test.go:TestGetSymbolAtLocation` | — (4b) |

> 依赖：`tsgo_compiler`(P6)/`tsgo_tsoptions`(P6)/`tsgo_bundled`(P9)/`vfstest`(P1)。**这些是后续 phase**，故该测试实际能跑要等 program/host 就绪——标 `—`（在 4b 用更轻的桩 program，或推迟到 P6 能起真 program 时收口）。见"测试基建依赖"。

## `tracer_test.go`

### `TestTracerPushPreservesEndArgMutations`（单块）

> 验证 `Tracer.Push` 返回的 `pop()` 闭包在结束时**重新读取 args**（end 阶段对 args 的后续修改要体现到 trace），且 `Push` 注入 `checkerId` 但不污染调用方的 args map（begin 事件不含调用方后加的 `variances`，end 事件含）。子阶段 **4a** 收口判据。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `tracer_push_preserves_end_arg_mutations` | begin/end 事件的 args 快照语义 + checkerId 注入 | 内存 vfs + `start_tracing(fsys,"/trace","",deterministic=true)`；`tracer=new_tracer(tr, 7)`；`pop = tracer.push(CheckTypes,"getVariancesWorker",args{id:1},true)`；push 后 `args` 不含 `checkerId`；改 `args["variances"]=["out"]` 后 `pop()`；`args` 仍不含 `checkerId`（未污染调用方）；停 trace 读 `/trace/trace.json` → begin 事件 `checkerId==7.0` 且 `variances==null`；end 事件 `checkerId==7.0` 且 `variances==["out"]` | `tracer_test.go:TestTracerPushPreservesEndArgMutations` | — (4a) |

> 依赖 `tsgo_tracing`(P1) + `vfstest`(P1) + `tsgo_json`(P1)，均已就绪 → 可在 4a 较早收口（不依赖后续 phase）。

## 正确性主体：P10 conformance 兜底（DEFER）

checker 的真正测试是 TypeScript 的 conformance 套件。按子阶段建议，每个子阶段在其 worklog 里登记"本阶段应让哪些 conformance 目录转绿"，作为该子阶段的真实收口判据。建议映射（目录名相对 `tests/cases/conformance/`）：

| 子阶段 | 建议覆盖的 conformance 目录（示例） | 完成 |
|---|---|---|
| 4c 声明类型 | `types/`(基础)、`interfaces/`、`classes/`(声明)、`enums/` | — (P10) |
| 4d 实例化+关系 | `types/typeRelationships/`、`generics/`、`types/typeParameters/` | — (P10) |
| 4e 推断 | `types/typeInference/`、`generics/`(调用推断) | — (P10) |
| 4f 控制流 | `controlFlow/`、`narrowing/` | — (P10) |
| 4g 表达式/语句 | `expressions/`、`statements/`、`functions/` | — (P10) |
| 4h JSX | `jsx/` | — (P10) |
| 4i 语法检查 | `grammar*` / 各处 grammar 错误 baseline | — (P10) |
| 4j node builder/序列化 | `declarationEmit/`（`.d.ts` baseline）、quickinfo fourslash | — (P10) |
| 4k emit resolver | declaration transformer baseline（经 P5） | — (P10) |

> P10 对拍方式：以 Go 的 `tsc` baseline（`.errors.txt` / `.types` / `.d.ts`）为 ground truth，Rust checker 产出逐字节/逐诊断对齐（诊断顺序经稳定排序）。

## 测试基建依赖（说明）

- `TestGetSymbolAtLocation` 与 `BenchmarkNewChecker` 需要一个可运行的 `Program`（来自 `compiler`/`tsoptions`/`bundled`，分布在 P6/P9）。在 P4 阶段，要么用**最小桩 program**（只实现 checker 需要的 `Program` trait 子集 + 手喂 bound source file）提前跑通 `get_symbol_at_location`，要么把这两个测试的"真 program 版本"推迟到 P6。推荐：4b 用桩 program 跑 `get_symbol_at_location` 的核心路径，P6 再加真 program 集成测试。
- `TestTracerPushPreservesEndArgMutations` 只依赖 `tracing`/`vfstest`/`json`（P1），无需 program → 4a 即可收口。

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*`（2 个）都已映射（`TestGetSymbolAtLocation`→4b、`TestTracerPushPreservesEndArgMutations`→4a）
- [ ] 表驱动子用例 —— N/A（两测试均单块）
- [ ] expected 值均取自 Go 测试字面量（如 `checkerId==7.0`、`variances==["out"]`）
- [ ] 每条带 `// Go:` 锚点
- [ ] 与 impl.md 双向对齐：`get_symbol_at_location`(4b)、`Tracer`(4a) 实现 TODO 承载这两个单测；其余子系统的正确性以"P10 conformance 目录"形式登记在上表

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `BenchmarkNewChecker` | 需真 program + TS 子模块源；仅性能 | P10 |
| `TestGetSymbolAtLocation` 真 program 版 | 需 `compiler`/`tsoptions`/`bundled` | P6（4b 先用桩 program） |
| 全部类型检查正确性（conformance/fourslash/`.d.ts`） | checker 测试的本体 | P10（按上表分子阶段登记） |
