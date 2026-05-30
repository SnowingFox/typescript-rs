# tracing: 实现方案（impl.md）

> **phase 归属（依赖序修正）**：本包**前移到 P4**（原列 P6）。原因：`checker` 非测试依赖 `tsgo_tracing`（`tracer.go`），须早于 checker（仅依赖 ast/json/scanner/tspath/vfs，近叶子）。详见根 README「依赖序口径」。

**crate**：`tsgo_tracing`　**目标**：记录编译器的 Chrome Trace Event（`trace.json`）+ 每个 checker 的类型快照（`types_<N>.json`）+ 索引（`legend.json`），用于 `--generateTrace` 性能/类型分析。
**依赖（crate）**：`tsgo_ast`（`Symbol`/`Node`/`EscapeAllInternalSymbolNames`）、`tsgo_json`、`tsgo_scanner`（行列换算）、`tsgo_tspath`、`tsgo_vfs`；外部 `xxh3`（线程 ID 稳定哈希，对应 Go `github.com/zeebo/xxh3`）。
**Go 源**：`internal/tracing/`（1 个非测试文件：`tracing.go` 764 行）

## 这个包是什么（业务说明）

`tracing` 是编译器的可观测性层。当用户传 `--generateTrace <dir>` 时，编译管线（`compiler` 包，本 phase）在 program 构建 / parse / bind / check / emit 各阶段调用 `Tracing.Push`/`Instant` 记录事件，最终落地三类文件：

1. **`trace.json`**：Chrome Trace Event 格式的 JSON 数组。事件类型 `M`(metadata)/`I`(instant)/`B`(begin)/`E`(end)/`X`(complete)，可在 `chrome://tracing` 或 Perfetto 里打开看火焰图。
2. **`types_<checkerId>.json`**：每个 checker（本 phase 的 `checkerPool` 给每个 checker 分配一个 `typeTracer`）把它创建的类型 dump 成 `TypeDescriptor` 数组，供分析"哪些类型最贵 / 递归身份 / 别名链"。
3. **`legend.json`**：上面两类文件的索引（configFilePath / tracePath / typesPath / checkerId），按 `typesPath` 排序后写出。

它和 checker 的关系靠**接口解耦**（`TracedType` 接口），这样 `tracing` 不直接 import `checker`，避免循环依赖——`checker` 实现 `TracedType`，`tracing` 只通过接口读取。这点在 Rust 侧用 **trait** 表达。

它处在 Phase 6，因为它依赖 `ast`（P2）/`scanner`（P3），且被 `compiler`（本 phase）和 checker 的调用点串起来；其测试是确定性并发（多 goroutine Push → 线程 ID 分配）的小型 gate。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5/§6。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Tracer` interface（`RecordType`/`DumpTypes`） | `trait Tracer` | 每个 checker 持有一个 `Box<dyn Tracer>`，避免 checker↔tracing 循环依赖 |
| `TracedType` interface（~30 个访问器） | `trait TracedType` | checker 的 `Type` 实现它；tracing 只读。Rust 用 `&dyn TracedType` 或泛型 `T: TracedType` |
| `RecursionIdentity() any` → `map[any]int` | `RecursionIdentity` 返回稳定可哈希 key（如 `TypeId`/枚举），`FxHashMap<RecursionId, usize>` | **存疑偏离**：Go 用 `any` 做 map key 靠接口相等；Rust 需要一个具体可 `Hash+Eq` 的判别 key。见偏离 |
| `traceEvent`（JSON tag + `omitzero`） | `#[derive(Serialize, Deserialize)] struct TraceEvent` + `#[serde(skip_serializing_if)]` | `Dur *float64` → `Option<f64>`；`Args map[string]any` → `Option<IndexMap<String, serde_json::Value>>`（顺序确定） |
| `map[string]any`（事件 args） | `IndexMap<String, ArgValue>` 或 `serde_json::Map` | **args 顺序影响 JSON 输出且测试断言**，用有序 map；值是判别联合（string/int/bool/float）见下 |
| `sync.Mutex` + `strings.Builder` 缓冲 | `Mutex<TraceState>`（含 `String` buffer + `flush_err`） | 单锁保护写缓冲/线程表/计数器 |
| `atomic.Bool traceStarted` | `AtomicBool` | nil-receiver 早退在 Rust 用 `Option<&Tracing>` 包装或方法守卫 |
| `time.Time` / `time.Since` | `std::time::Instant` | `deterministic` 模式不取真实时间（见下） |
| `xxh3.New().Sum64()` | `xxh3` crate（`xxh3_64`） | 文件线程 ID 的稳定哈希，必须与 Go 同算法同种子才能字节对齐（实际只在非确定性路径用） |
| nil-receiver 安全（`tr == nil`） | `impl Tracing` 的方法都先判 `started`；调用方持 `Option<Arc<Tracing>>`，`None` 即 no-op | Go 大量 `tr.Push(...)`（tr 可能 nil）；Rust 改成 `if let Some(tr) = ...` 或在 `Option<&Tracing>` 上加 ext trait |

### 事件 args 值类型（判别联合）

Go 的 `map[string]any` 值实际只有几种：`string`（path/fileName/name/...）、`int`（checkerId/id/refKind/count）、`bool`（hasResolved）、以及反序列化回来后的 `float64`（JSON number）。Rust 侧定义：

```rust
enum ArgValue { Str(String), Int(i64), Bool(bool), Float(f64) }
```

> 注意测试里 `findEvent(..., "id", float64(1))`：JSON 反序列化后整数变 `float64`，所以**比较时统一按 JSON 解析后的类型**。Rust 反序列化用 `serde_json::Value`（Number 统一），断言侧按 `Value` 比较即可，避免 int/float 歧义。

### 并发模型（本包的小并发点）

- `Tracing` 是被多线程（多 checker / 多文件 parse goroutine）共享的单实例。`Push(separateBeginAndEnd=true)` 返回一个 `end` 闭包，begin 与 end 可能在不同线程、交错调用。
- 所有写缓冲（`traceContent`）、线程 ID 表（`threadIDs`/`threadKeys`）、确定性计数器（`timestampCounter`）都由 `tr.mu` 保护。Rust 用 `Mutex<TraceState>`。
- **线程 ID 稳定性是核心不变量**（测试 `TestThreadIDsAreStableAcrossFirstSeenOrder`）：同一组路径无论 begin 顺序如何，分配到的 TID 必须一致。算法 = 按 `traceThreadKey` 的 `defaultThreadID()`（checker 用 `firstSyntheticThreadID+index`，文件用 `stableTraceThreadID` 的 xxh3 哈希落到 `[firstFileThreadID, +1e9)`），冲突则线性探测 `tid++`。
- `DumpTypes` 故意在**持锁前**调用（`StopTracing` 注释：`Display()` 会重入 checker→`Push/Pop`，需要 `tr.mu`，所以先 dump 再加锁）。Rust 移植必须保留这个"先 dump、后锁"的顺序，否则死锁。
- 闭包返回值（`func()`）在 Rust 用 `impl FnOnce()` 或返回一个实现 `Drop` 的 guard（`PushGuard`）。**推荐返回 guard 用 RAII** 对齐 Go 的 `defer end()`，但要保证 guard 不在 begin 之前的早退路径里 panic。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/tracing/tracing.go` | `internal/tracing/lib.rs`（basename `tracing` == crate 目录名 → `lib.rs`） | 全部内容：`Tracer`/`TracedType` trait、`Tracing`、`typeTracer`、事件/描述符 struct、线程 ID 逻辑 |

## 依赖白名单（本包新增的 crate）

- `xxh3`（最新稳定版，执行期 `cargo add xxh3`）——文件线程 ID 稳定哈希。记到 `references/crate-map.md`。
- `indexmap`（已在 §10）——事件 args 有序 map。
- `serde`/`serde_json`（经 `tsgo_json`）——事件/描述符序列化。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按 TDD 推进序。每条 `[ ]`，实现后改 `[x]`。

### `lib.rs`（Go: `internal/tracing/tracing.go`）

**Trait（解耦 checker）**

- [x] `pub trait Tracer { fn record_type(&self, t: Box<dyn TracedType + Send + Sync>); fn dump_types(&self) -> Result<(), TraceError>; }`　`// Go: tracing.go:Tracer`
- [x] `pub trait TracedType { ... ~30 个访问器 ... }`（`id`/`format_flags`/`is_conditional`/`symbol`/`alias_symbol`/`alias_type_arguments`/各 type-specific 访问器/`display`；node 访问器返回 `Option<NodeId>`）　`// Go: tracing.go:TracedType`

**核心类型**

- [x] `pub struct Tracing<'fs> { ... }`（fs: `&(dyn Fs + Send + Sync)`/traceDir/tracePath/configFilePath + `Mutex<TraceState>`{legend/tracers/缓冲/线程表/计数器/flushErr} + metadataTs/deterministic/startTime/`AtomicBool`）　`// Go: tracing.go:Tracing`
- [x] `pub enum Phase { Parse, Program, Bind, Check, CheckTypes, Emit, Session }`（`as_str()` 返字符串字面量，用于事件 `cat`）　`// Go: tracing.go:Phase`
- [x] `pub struct TraceRecord { config_file_path, trace_path, types_path, checker_id }`（serde，`omitzero`→`skip_serializing_if`）　`// Go: tracing.go:TraceRecord`
- [x] `struct TraceEvent { pid, tid, ph, cat, ts, name, s, dur: Option<f64>, args: Option<Args> }`（serde；crate-private 如 Go）　`// Go: tracing.go:traceEvent`

**会话生命周期**

- [x] `pub fn start_tracing(fs, trace_dir, config_file_path, deterministic: bool) -> Result<Tracing, TraceError>` — 写 `[` + 3 条 metadata 事件 + `WriteFile` 截断初始化　`// Go: tracing.go:StartTracing`
- [x] `fn timestamp(&mut self, deterministic, start_time) -> f64` — deterministic 时 `++counter`，否则 `start_time.elapsed()` 纳秒/1000　`// Go: tracing.go:timestamp`
- [x] `pub fn stop_tracing(&self) -> Result<(), TraceError>` — **先**逐 tracer `dump_types()`（无锁），**再**加锁：flush 剩余 + 写 `\n]\n`、写排序后的 `legend.json`　`// Go: tracing.go:StopTracing`

**事件写入**

- [x] `fn write_event(&mut self, e: &TraceEvent)`（`marshal` 到 buffer；args 为 `BTreeMap` 故 key 已排序，等价 Go `Deterministic(true)`）　`// Go: tracing.go:writeEvent/writeEventTo`
- [x] `fn maybe_flush_locked(&mut self, fs, trace_path)` — buffer 超 `FLUSH_THRESHOLD`(256KiB) 则 `AppendFile`；flushErr 已置则丢弃缓冲并 no-op　`// Go: tracing.go:maybeFlushLocked`
- [x] `pub fn instant(&self, phase, name, args)` — 写 `,\n` + `I` 事件（`s="g"`）；未启动早退；锁内二次检查 started　`// Go: tracing.go:Instant`
- [x] `pub fn push(&self, phase, name, args, separate_begin_and_end: bool) -> Box<dyn FnOnce() + '_>`（闭包，对齐 Go `func()`）— 见下分支　`// Go: tracing.go:Push`
  - 分支 A `separate_begin_and_end=true`：立即写 `B`，返回写 `E` 的闭包（同一 tid，闭包内重取锁）。
  - 分支 B（采样）`deterministic`：返回 no-op 闭包（确定性模式跳过采样事件以免 flaky）。
  - 分支 C（采样）非确定性：clone args、记 startTime，闭包计算 dur，仅当跨 10ms 采样边界才写 `X` 事件。

**线程 ID 分配（稳定性命门）**

- [x] `fn thread_id_locked(&mut self, args, metadata_ts) -> i32` — 查/建 `TraceThreadKey`，分配后写一条 `thread_name` metadata；冲突线性探测　`// Go: tracing.go:threadIDLocked`
- [x] `fn write_thread_name_event_locked(&mut self, tid, name, metadata_ts)`　`// Go: tracing.go:writeThreadNameEventLocked`
- [x] `struct TraceThreadKey { kind, text, index, has_index }` + `enum TraceThreadKind { Checker, File }`（`Hash + Eq`）　`// Go: tracing.go:traceThreadKey/traceThreadKind`
- [x] `fn trace_thread_key_from_args(args) -> Option<TraceThreadKey>` — 优先 `checkerId`(int)，否则按 `["path","fileName","containingFileName","jsFilePath","declarationFilePath"]` 顺序取首个非空 string　`// Go: tracing.go:traceThreadKeyFromArgs`
- [x] `fn default_thread_id(&self) -> i32`（checker+有 index+>=0 → `FIRST_SYNTHETIC_THREAD_ID+index`，否则 `stable_trace_thread_id`）　`// Go: tracing.go:defaultThreadID`
- [x] `fn display_name(&self) -> String`（`"checker:0"` / `"file:/a.ts"`）　`// Go: tracing.go:displayName`
- [x] `fn stable_trace_thread_id(key) -> i32` — `xxh3::hash64_with_seed(kind + ":" + (index|text), 0)`，`FIRST_FILE_THREAD_ID + sum64 % 1e9`（数值对拍 DEFER P10）　`// Go: tracing.go:stableTraceThreadID`
- [x] 常量：`MAIN_THREAD_ID=1`/`FIRST_SYNTHETIC_THREAD_ID=2`/`FIRST_FILE_THREAD_ID=1_000_000`/`FILE_THREAD_ID_HASH_RANGE=1_000_000_000`/`SAMPLE_INTERVAL_MICROS=10_000`/`TRACE_FILE_NAME="trace.json"`/`FLUSH_THRESHOLD=256*1024`/`TRACE_THREAD_ARG_KEYS`　`// Go: tracing.go`（const/var 块）

**每-checker 类型 tracer**

- [x] `struct TypeTracer<'fs> { fs, types_path, types: Mutex<Vec<Box<dyn TracedType + Send + Sync>>> }`（crate-private 如 Go；checker_index 不需保留）　`// Go: tracing.go:typeTracer`
- [x] `fn new_type_tracer(&self, checker_index: i32) -> Arc<dyn Tracer + Send + Sync + '_>`（在 `Tracing` 上；追加 legend 条目 + tracers）　`// Go: tracing.go:NewTypeTracer`
- [x] `impl Tracer for TypeTracer::record_type`（锁内 push）　`// Go: tracing.go:typeTracer.RecordType`
- [x] `impl Tracer for TypeTracer::dump_types` — `mem::take` types→释放锁→逐个 `build_type_descriptor`→`marshal`→以 `[`开头、`,\n`分隔、`]\n`结尾写文件（**"`[`后不换行"以使 type id == 行号**）　`// Go: tracing.go:typeTracer.DumpTypes`

**类型描述符**

- [x] `pub struct TypeDescriptor { ... 30+ 字段 ... }`（serde `rename_all="camelCase"`；`conditionalTrueType/FalseType` 是 `Option<i32>`，未解析分支序列化为 `-1`；location 字段 DEFER P10）　`// Go: tracing.go:TypeDescriptor`
- [x] `pub struct Location { path, start: Option<LineAndChar>, end }` + `pub struct LineAndChar { line, character }`（1-indexed）　`// Go: tracing.go:Location/LineAndChar`
- [x] `pub fn build_type_descriptor(typ: &dyn TracedType, recursion_identity_map: &mut FxHashMap<RecursionId, usize>) -> TypeDescriptor`　`// Go: tracing.go:buildTypeDescriptor`
  - 已实现：递归身份 token（首见 = `map.len()`）；intrinsic/symbol 名（优先 aliasSymbol，`escape_all_internal_symbol_names`）；tuple/union/intersection/aliasArgs/keyof/indexedAccess/conditional(含 -1 分支)/substitution/reference(target+typeArgs)/reverseMapped/evolvingArray/display。
  - **DEFER(phase-10)**：location 派生字段（`reference_location`/`destructuring_pattern`/`first_declaration`）走 `get_location` 故恒 `None`。
- [x] `fn map_type_ids(types: &[&dyn TracedType]) -> Vec<u32>`（调用点 `Some(...)` 包裹）　`// Go: tracing.go:mapTypeIds`
- [x] `fn get_location(node: NodeId) -> Option<Location>` — **DEFER(phase-10) stub 恒返 `None`**；blocked-by：`tsgo_ast` 无 `get_source_file_of_node`、`tsgo_scanner` 推迟 `GetECMALine*`/`GetTokenPosOfNode`。wiring 已就位，落地仅改函数体　`// Go: tracing.go:getLocation`

### Cargo / crate 接线

- [x] `internal/tracing/Cargo.toml`（`name = "tsgo_tracing"`，path deps：`tsgo_ast` `tsgo_json` `tsgo_scanner` `tsgo_tspath` `tsgo_vfs`；外部 `serde`(derive) `rustc_hash` `xxh3`。**偏离**：未用 `indexmap`（事件 args 改 `BTreeMap` 已满足排序确定性）；`serde_json` 经 `tsgo_json` 复用）
- [x] 根 `Cargo.toml` workspace members 追加（脚手架已就位）
- [x] `lib.rs` 公开 `Tracing`/`Tracer`/`TracedType`/`Phase`/`TypeDescriptor`/`Location`/`LineAndChar`/`TraceRecord`/`TraceError`/`ArgValue`/`Args`/`RecursionId`/`build_type_descriptor`

## TDD 推进顺序（tracer bullet → 增量）

1. `start_tracing` → `stop_tracing` 空会话：产出含 3 条 metadata 事件 + `]` 的合法 `trace.json` + 空 `legend.json`（tracer bullet）。
2. `push(separate=true)` 单事件对：begin/end 同 tid，well-nested（对应 `assertDurationEventsAreWellNestedByThread` 的最小情形）。
3. 线程 ID 分配：两个文件路径分到不同 tid，且写出对应 `thread_name`（对应 `TestConcurrentDurationEventsUseSeparateThreadIDs` 文件部分）。
4. checker tid：`checkerId=0` → `FIRST_SYNTHETIC_THREAD_ID`，`thread_name="checker:0"`。
5. 稳定性：同组路径不同 begin 顺序 → 相同 tid 映射（对应 `TestThreadIDsAreStableAcrossFirstSeenOrder`）。
6. `TypeTracer.dump_types`：递归身份 token、`-1` 条件分支、`[`不换行等行为（行为级补充，见 tests.md）。

## 与 Go 的已知偏离（divergence）

- **nil-receiver → `Option`/guard**：Go 全程 `tr.Push(...)`（tr 可 nil）。Rust 改为调用方持 `Option<Arc<Tracing>>` 并 `if let Some(tr)`，或为 `Option<&Tracing>` 加 ext-trait 方法，使 `None` 成 no-op。结构等价。
- **`RecursionIdentity() any` → 具体 key**：Go 用 `any` 做 map key（接口相等）。Rust 需 checker 暴露一个 `Hash+Eq` 的稳定判别（如类型对象的 arena id 或一个 `RecursionId` newtype）。这是为了零 `unsafe`/可哈希的必要偏离，必须保证"同一递归身份得到同一 token"的不变量。
- **`map[string]any` → 有序判别联合**：args 改 `IndexMap<String, ArgValue>`（写出顺序确定）。反序列化比较时按 `serde_json::Value`（数字统一），对齐测试里 `float64(1)` 的现象。
- **闭包返回 → RAII guard**：`Push` 返回 `impl FnOnce()` 或 `PushGuard`（`Drop` 写 `E`）。用 guard 时注意早退路径（未启动）返回一个 no-op guard。
- **"先 dump 后锁"**：保留 `StopTracing` 的顺序（dump 可能重入 checker→`Push`），否则死锁。Rust 同样不能在持 `tr.mu` 时调用会回调 checker 的 `dump_types`。
- **xxh3 字节对齐**：`stable_trace_thread_id` 仅在非确定性路径生效；要与 Go 完全一致需同版本 xxh3 + 同输入。确定性测试不触发它，故 P6 gate 不依赖其精确值；真实 trace 的字节对拍归 P10。

## 转交 / 推迟（DEFER）

- `TracedType` 的具体实现由 **checker（P4）** 提供；本包只定义 trait + 描述符构建。`build_type_descriptor` 的真实数据要等 checker 落地后才能端到端验证，故类型 dump 的字节级对拍标 `// DEFER(phase-10)`。
- 真实 `--generateTrace` 的 `trace.json`/`types_*.json` 与 Go 字节对拍归 **P10 parity**（涉及真实时间戳→必须 deterministic 模式 + checker 全实现）。
