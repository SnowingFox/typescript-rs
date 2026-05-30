# tracing: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 文件 / 2 `func Test` / 共 2 顶层用例（无表驱动子用例，但每个用例内含多组断言）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/tracing/tracing_test.go` | `internal/tracing/lib_test.rs`（兄弟文件，`use super::*;`，经 `#[cfg(test)] #[path="lib_test.rs"] mod tests;` 挂载） | 2 |

测试用 `vfstest.FromMap`（内存 FS，`useCaseSensitiveFileNames=true`）+ deterministic 模式启动，写出后读回 `trace.json`，`json.Unmarshal` 成 `[]traceEvent` 再断言。Rust 侧对应 `tsgo_vfs` 内存 FS + `serde_json` 反序列化为 `Vec<TraceEvent>`。

## `tracing_test.go`

> 这两个 Test 不是表驱动；下表逐 Test 拆成"该 Test 内的每条独立断言"作为子用例行，便于 Rust 侧 1:1 复原。

### TestConcurrentDurationEventsUseSeparateThreadIDs

构造：`StartTracing(fs,"/trace","",true)`；按序 `Push(Parse,"createSourceFile",{path:"/a.ts"},true)`/`{path:"/b.ts"}`、`endA()`、`endB()`；再 `Push(Check,"checkSourceFile",{checkerId:0,path:"/a.ts"},true)`、`Push(CheckTypes,"getVariancesWorker",{checkerId:0,id:1},true)`、`endVariance()`、`endCheck()`；`StopTracing()`；读回 `/trace/trace.json`。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `a_begin_end_same_tid` | `/a.ts` 的 B 与 E 事件同 TID | `aBegin.TID == aEnd.TID` | `tracing_test.go:TestConcurrentDurationEventsUseSeparateThreadIDs` | ✓ |
| `b_begin_end_same_tid` | `/b.ts` 的 B 与 E 事件同 TID | `bBegin.TID == bEnd.TID` | 同上 | ✓ |
| `a_and_b_different_tid` | 两个不同文件分到不同 TID | `aBegin.TID != bBegin.TID` | 同上 | ✓ |
| `a_thread_name` | `/a.ts` 的 tid 有 `thread_name` metadata | `thread_name == "file:/a.ts"` | 同上（`assertThreadName`） | ✓ |
| `b_thread_name` | `/b.ts` 的 tid 有 `thread_name` metadata | `thread_name == "file:/b.ts"` | 同上 | ✓ |
| `checker_and_variance_same_tid` | 同一 checker 的两事件同 TID | `checkBegin.TID == varianceBegin.TID` | 同上 | ✓ |
| `checker_thread_name` | checker tid 命名 | `thread_name == "checker:0"`（即 `FIRST_SYNTHETIC_THREAD_ID=2`） | 同上 | ✓ |
| `variance_arg_id_is_json_number` | 反序列化后 `id` 为 JSON number | `findEvent(...,"id", 1)` 命中（Go 写 `float64(1)`；Rust serde untagged 解析为 `Int(1)`） | 同上 | ✓ |
| `duration_events_well_nested_by_thread` | 每个 tid 上 B/E 严格配对、cat/name 匹配、栈清空 | 见 `assertDurationEventsAreWellNestedByThread` | 同上 | ✓ |

> 映射说明：上述 9 条断言由 3 个 Rust `#[test]` 覆盖——`distinct_files_get_distinct_thread_ids`（前 5 行）、`checker_events_share_thread_id_and_json_number_arg`（checker/variance/json-number 三行）、`push_begin_end_pair_well_nested` + 各用例内 `assert_well_nested`（well-nested 行）。

### TestThreadIDsAreStableAcrossFirstSeenOrder

构造：`traceThreadIDsForPaths(["/a.ts","/b.ts"])` 与 `traceThreadIDsForPaths(["/b.ts","/a.ts"])`，各自跑独立 deterministic 会话，收集 `path -> TID` map。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `thread_ids_are_stable_across_first_seen_order` | 路径→TID 映射与 begin 顺序无关 | `map(["/a","/b"]) == map(["/b","/a"])`（`assert_eq!`） | `tracing_test.go:TestThreadIDsAreStableAcrossFirstSeenOrder` | ✓ |

### 测试辅助（需在 Rust 测试模块复刻）

| Go 辅助 | Rust 对应 | 说明 |
|---|---|---|
| `findEvent(events, ph, name, argName, argValue)` | helper：扫 `Vec<TraceEvent>` 找 `ph && name && args[argName]==argValue` | argValue 用 `serde_json::Value` 比较（数字统一） |
| `assertThreadName(events, tid, name)` | helper：找 `ph=="M" && name=="thread_name" && tid==tid && args["name"]==name` | |
| `assertDurationEventsAreWellNestedByThread(events)` | helper：按 tid 维护栈，B 压栈、E 出栈校验 cat/name、结束时所有栈空 | |
| `traceThreadIDsForPaths(paths)` | helper：跑会话→读回→收集 `path->TID` | |

## 行为级补充测试（impl.md 有 TODO，Go 无直接单测覆盖）

`typeTracer`/`build_type_descriptor`/`get_location`/采样事件/flush 在 Go 侧无直接单测（其正确性由真实 `--generateTrace` 输出在 P10 兜底）。本轮补充少量行为级用例（expected 取自 Go 实现的确定行为）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `empty_session_well_formed_json` | 空会话产出合法 trace.json | start→stop，无事件 → 含 3 条 metadata 事件 + `]` 结尾，可被 `serde_json` 解析 | tracing.go:StartTracing/StopTracing | ✓ |
| `metadata_events_present` | 头部三条 metadata | `process_name`(name=tsgo) / `thread_name`(Main) / `TracingStartedInBrowser`，均 `pid=1 tid=1 ph="M"` | tracing.go:StartTracing | ✓ |
| `legend_sorted_by_types_path` | legend 按 typesPath 排序 | 多 tracer（创建序 2,0,1）→ `legend.json` 条目按 typesPath 字典序（0,1,2） | tracing.go:StopTracing（`slices.SortFunc`） | ✓ |
| `instant_event_scope_global` | instant 事件带 `s="g"` | `instant(...)` → 事件 `ph="I" s="g"` | tracing.go:Instant | ✓ |
| `deterministic_skips_sampled_events` | deterministic 下 `push(...,false)` 不产生事件 | `push(Parse,"x",None,false)`+end → trace 无新增 X 事件 | tracing.go:Push（deterministic 分支返回 no-op） | ✓ |
| `deterministic_timestamps_monotonic` | deterministic 时间戳为单调计数 | 连续 B/E 事件 ts 递增整数 | tracing.go:timestamp | ✓ |
| `type_descriptor_unresolved_conditional_minus_one` | 未解析条件分支序列化为 -1 | 假 `TracedType`（conditional，true/false=None）→ `conditionalTrueType=-1`,`conditionalFalseType=-1` | tracing.go:buildTypeDescriptor | ✓ |
| `type_descriptor_recursion_token_stable` | 同递归身份得同 token | 两个 type 同 recursionIdentity(42) → 同 `recursionId`(0) | tracing.go:buildTypeDescriptor | ✓ |
| `dump_types_open_bracket_no_newline` | `[` 后不换行（type id == 行号） | dump 2 个类型 → 输出以 `[` 紧跟首个对象、`,\n` 分隔、`]\n` 结尾 | tracing.go:DumpTypes | ✓ |
| `flush_threshold_appends` | 缓冲超阈值触发 AppendFile | 3000 个 B/E 对使缓冲 >256KiB → 文件被增量写且不丢事件（回读计数 3000+3000、well-nested） | tracing.go:maybeFlushLocked | ✓ |

> `stable_trace_thread_id` 的精确数值（xxh3）不在 Rust 单测断言绝对值——确定性会话里文件 tid 仍走 `stableTraceThreadID`，但测试只断言"稳定/互异/有命名"，不断言具体数字（与 Go 数值对拍归 P10）。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（2 个）都已映射
- [x] 两个非表驱动 Test 内的每条独立断言都已逐行列出
- [x] expected 值均取自 Go 测试字面量（`file:/a.ts`/`checker:0`/`float64(1)` 等）
- [x] 每条带 `// Go:` 锚点（表内"Go 对照"列）
- [x] 与 impl.md 双向对齐：测试涉及的 `push`/`thread_id`/`start_tracing`/`stop_tracing`/`build_type_descriptor` 在 impl.md 均有实现 TODO

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 真实 `trace.json` 与 Go 字节级对拍 | 需 compiler 全管线 + deterministic | P10 parity |
| `types_<N>.json` 与 Go 字节级对拍 | 需 checker（P4）实现 `TracedType` 真实数据 | P10 parity |
| `stable_trace_thread_id` 与 Go xxh3 数值一致 | 非确定性路径，需同 xxh3 实现 | P10 parity |
| `get_location` 行列换算与 Go 一致 | 需 scanner（P3）的 ECMA 行列换算落地 | P3 / P10 |
