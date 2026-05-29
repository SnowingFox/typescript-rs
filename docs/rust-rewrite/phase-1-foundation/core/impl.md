# core: 实现方案（impl.md）

**crate**：`tsgo_core`　**目标**：编译器的"地基核心"——通用切片/映射工具、文本位置模型（`TextPos`/`TextRange`/`TextChange`）、并发原语（`WorkGroup`/`Semaphore`/`ThrottleGroup`）、arena 分配器、`CompilerOptions` 及全部相关枚举（`ModuleKind`/`ScriptTarget`/`Tristate` 等）、拼写建议（Levenshtein）、并行 BFS 等。
**依赖（crate）**：`tsgo_collections`、`tsgo_stringutil`、`tsgo_tspath`、`tsgo_debug`、`tsgo_json`（必须在它们之后落地）。外部：`rayon`/`crossbeam-channel`、`rustc_hash`，可能 `typed-arena`/`la-arena`。
**Go 源**：`internal/core/`（28 个非测试文件，其中 5 个为 `*_stringer_generated.go`）

## 这个包是什么（业务说明）

`core` 是整个 typescript-go 的底层公共库，几乎被 P2 之后的所有包依赖。它聚合了七类能力：

1. **通用集合/函数式工具**（`core.go`，~70 个泛型函数）：`Filter`/`Map`/`Some`/`Every`/`Find`/`Concatenate`/`Splice`/`Deduplicate`/`DiffMaps` 等，对齐 TS 里的数组/对象工具；还有 `Memoize`/`IfElse`/`OrElse`/`Coalesce`、行起点计算 `ComputeECMALineStarts`、UTF-16 长度 `UTF16Len`、拼写建议 `GetSpellingSuggestion`（带 Levenshtein）。
2. **文本模型**（`text.go`/`textchange.go`）：`TextPos`(i32)、`TextRange`(pos/end + 一堆包含/重叠判定)、`TextChange` + `ApplyBulkEdits`。
3. **选项与枚举**（`compileroptions.go` 等）：`CompilerOptions`（~120 字段）+ 派生 getter、`ModuleKind`/`ModuleResolutionKind`/`ScriptTarget`/`JsxEmit`/`NewLineKind`/`ModuleDetectionKind` 枚举、`BuildOptions`/`WatchOptions`/`TypeAcquisition`/`ParsedOptions`/`ProjectReference`、`Tristate`。
4. **数据结构**：`Arena[T]`（bump 分配器）、`LinkStore[K,V]`（懒分配链接存储）、`Stack[T]`、`Pattern`（`*` 通配匹配）、并行 BFS（`bfs.go`）、`BinarySearchUniqueFunc`。
5. **并发**（`workgroup.go`/`semaphore.go`）：`WorkGroup`（单线程/并行两实现）、`ThrottleGroup`（带信号量限流的 errgroup）、`Semaphore`（Unlimited/Limited）。
6. **杂项**：`context.go`（request ID 携带）、`version.go`（编译器版本）、`nodemodules.go`（Node 内置模块表）、`ScriptKind`、`GetScriptKindFromFileName`。

因依赖 collections/stringutil/tspath/debug/json，它必须在这 5 个包**之后**落地，是 Phase 1 的收口前置核心。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5/§6。本包关键决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| 泛型工具 `Filter[T]/Map[T,U]/...` | 泛型 `fn` + 闭包，或 `Iterator` 适配器 | 多数可用标准库迭代器（`iter().filter().collect()`）；但为 1:1 保留签名与零值语义，建议同名 `fn`。返回切片→`Vec<T>` |
| `*new(T)`（零值） | `T::default()` | `FirstOrNil`/`Find` 等返回零值；Rust 用 `Option<T>` 更地道，但为对齐 Go 语义可用 `Default` |
| `Arena[T]`（`[]T` + 返回 `*T` 稳定指针） | `typed-arena::Arena<T>` 或 `id-arena`/索引 | **关键**：Go 的 `Arena.New()` 满了就分配新 backing（旧指针仍有效）。Rust 返回 `&mut T` 到 `Vec` 不健全 → 用 `typed-arena`（稳定 `&T`）或索引化（`la-arena` + `Idx`）。零 unsafe |
| `LinkStore[K,V]`（`map[K]*V` + Arena） | `HashMap<K, Idx>` + arena，或 `HashMap<K, Box<V>>` | 懒分配 V，返回稳定引用；用 arena 索引或 `Box` |
| `TextPos int32` / `TextRange{pos,end}` | `struct TextPos(i32)` / `struct TextRange{pos:TextPos, end:TextPos}` | newtype；`UndefinedTextRange` = (-1,-1) |
| `Tristate byte`（Unknown/False/True） | `enum Tristate { Unknown, False, True }`（`#[repr(u8)]`） + JSON | `MarshalJSON`: True→`true`/False→`false`/Unknown→`null`；`UnmarshalJSON` 反之 |
| `CompilerOptions`（120 字段，reflect Clone） | `#[derive(Clone, Default)] struct CompilerOptions{...}` | **去 reflect**：Go 用 reflect 做 shallow Clone；Rust `#[derive(Clone)]` 直接。JSON 用 serde + `#[serde(rename, skip_serializing_if)]`（对齐 `omitzero`） |
| `ModuleKind`/`ScriptTarget`/... `int32` + iota/显式值 | `#[repr(i32)] enum`（保留显式判别值，如 `ESNext=99`/`Node16=100`） | 数值不连续（`ESNext=99`），必须保留显式值以对齐范围比较（`>= ScriptTargetES2022`） |
| `*_stringer_generated.go`（Stringer） | `impl Display for <Enum>`（手写或 `strum`/`derive`） | 生成的 `String()` → Rust `Display`；非默认前缀（trimprefix）需对齐输出文本 |
| `WorkGroup`（goroutine/单线程双实现） | `trait WorkGroup` + 并行(`rayon`/`std::thread::scope`)/单线程实现 | PORTING §6：数据并行 rayon；保确定性 |
| `ThrottleGroup`（errgroup + chan 信号量） | `std::thread::scope` + `crossbeam-channel` 信号量 + 收集首个 error | `errgroup` → 自写错误收集 |
| `Semaphore`（chan struct{}） | `trait Semaphore` + `std::sync` 信号量（`Semaphore`/`Mutex+Condvar` 或 `tokio::sync` 不用）；Limited 用有界 channel | `Acquire` 返回 release 闭包 → Rust 返回 RAII guard |
| `context.Context`（WithRequestID） | 显式传 `request_id: Option<&str>` | PORTING §3：不引入 async context；request ID 显式下传 |
| goroutine + atomic（bfs.go） | `rayon`/`std::thread::scope` + `AtomicI64`/`AtomicBool` | 并行 BFS：原子最小值（`updateMin` CAS 循环）；输出确定性靠 OrderedMap |
| `sync.Pool`（levenshtein buffers） | `thread_local!` 缓冲 或每次分配 | 复用缓冲优化；Rust 用 `thread_local!` 或直接分配 + `// PERF(port)` |
| `sync.OnceValue`（NodeCoreModules/version） | `LazyLock` / `OnceLock` | 一次性初始化 |
| `unicode/utf16`（UTF16Len） | `char::len_utf16` / 手写 | UTF-16 code unit 计数 |

### 所有权图：Arena / LinkStore（零 unsafe 落地点）

Go 的 `Arena[T]` 返回裸 `*T`，靠"满了分新 backing、旧 backing 不释放"保证指针稳定。Rust 两条安全路径：
- **`typed-arena::Arena<T>`**：`alloc(value) -> &mut T`，引用在 arena 存活期内稳定，零 unsafe。`LinkStore` 用 `HashMap<K, &'a V>`（生命周期绑 arena）或 `HashMap<K, Idx>`。
- **索引化（`la-arena`）**：`Arena<T>` + `Idx<T>`，所有引用用 `Idx`（与 AST 的 NodeId 模型一致，PORTING §5）。**推荐索引化**以与后续 AST/Symbol arena 统一。

该偏离写明即可（结构 1:1，引用语义从裸指针→arena 索引/借用）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/core/core.go` | `internal/core/lib.rs`（basename==目录名→`lib.rs`，crate 入口；亦可拆 `util.rs` 由 lib re-export） | ~70 个泛型工具 + 行起点 + UTF16Len + 拼写建议 |
| `internal/core/arena.go` | `internal/core/arena.rs` | `Arena[T]` |
| `internal/core/bfs.go` | `internal/core/bfs.rs` | 并行 BFS |
| `internal/core/binarysearch.go` | `internal/core/binarysearch.rs` | `BinarySearchUniqueFunc` |
| `internal/core/stack.go` | `internal/core/stack.rs` | `Stack[T]` |
| `internal/core/pattern.go` | `internal/core/pattern.rs` | `Pattern`（`*` 通配） |
| `internal/core/linkstore.go` | `internal/core/linkstore.rs` | `LinkStore[K,V]` |
| `internal/core/text.go` | `internal/core/text.rs` | `TextPos`/`TextRange` |
| `internal/core/textchange.go` | `internal/core/textchange.rs` | `TextChange`/`ApplyBulkEdits` |
| `internal/core/tristate.go` | `internal/core/tristate.rs` | `Tristate` + JSON |
| `internal/core/compileroptions.go` | `internal/core/compileroptions.rs` | `CompilerOptions` + 枚举 + getter |
| `internal/core/buildoptions.go` | `internal/core/buildoptions.rs` | `BuildOptions` |
| `internal/core/parsedoptions.go` | `internal/core/parsedoptions.rs` | `ParsedOptions` |
| `internal/core/watchoptions.go` | `internal/core/watchoptions.rs` | `WatchOptions` + 枚举 |
| `internal/core/typeacquisition.go` | `internal/core/typeacquisition.rs` | `TypeAcquisition` |
| `internal/core/projectreference.go` | `internal/core/projectreference.rs` | `ProjectReference` |
| `internal/core/scriptkind.go` | `internal/core/scriptkind.rs` | `ScriptKind` 枚举 |
| `internal/core/languagevariant.go` | `internal/core/languagevariant.rs` | `LanguageVariant` 枚举 |
| `internal/core/semaphore.go` | `internal/core/semaphore.rs` | `Semaphore` |
| `internal/core/workgroup.go` | `internal/core/workgroup.rs` | `WorkGroup`/`ThrottleGroup` |
| `internal/core/context.go` | `internal/core/context.rs` | request ID（改显式传参） |
| `internal/core/version.go` | `internal/core/version.rs` | `Version`/`VersionMajorMinor` |
| `internal/core/nodemodules.go` | `internal/core/nodemodules.rs` | Node 内置模块表 |
| `*_stringer_generated.go`（5 个） | 合并进各枚举的 `impl Display`（`tristate.rs`/`scriptkind.rs`/`languagevariant.rs`/`compileroptions.rs`） | 生成 Stringer → Rust `Display` |

## 依赖白名单（本包新增的 crate）

- `rayon` / `crossbeam-channel`（WorkGroup/ThrottleGroup/BFS 并行）。
- `typed-arena` 或 `la-arena`（Arena/LinkStore；与全仓 arena 选型统一）。
- `rustc_hash`（内部 map）、`serde`（CompilerOptions JSON）。
- 内部：`tsgo_collections` `tsgo_stringutil` `tsgo_tspath` `tsgo_debug` `tsgo_json`。
- 记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs` / 通用工具（Go: `internal/core/core.go`）

> ~70 个泛型函数；下面按族列，逐个带锚点。

- [x] 切片变换：`filter / filter_seq / filter_index / map / try_map / map_index / map_non_nil / map_filtered / flat_map / same_map / same_map_index`　`// Go: core.go:Filter/.../SameMapIndex`
- [x] 谓词/查找：`same / some / every / or / find / find_last / find_index / find_last_index / count_where`　`// Go: core.go:Same/Some/Every/Or/Find/FindLast/FindIndex/FindLastIndex/CountWhere`
- [x] 取元素：`first_or_nil / last_or_nil / element_or_nil / first_or_nil_seq / first_non_nil / first_non_zero`　`// Go: core.go:FirstOrNil/.../FirstNonZero`
- [x] 拼接/编辑：`concatenate / splice / replace_element / insert_sorted / append_if_unique / deduplicate / deduplicate_sorted / flatten / min_all_func`　`// Go: core.go:Concatenate/Splice/ReplaceElement/InsertSorted/AppendIfUnique/Deduplicate/DeduplicateSorted/Flatten/MinAllFunc`
- [x] 映射工具：`diff_maps / diff_maps_func / copy_map_into / unordered_equal`　`// Go: core.go:DiffMaps/DiffMapsFunc/CopyMapInto/UnorderedEqual`
- [x] 函数式：`memoize / if_else / or_else / coalesce / identity`　`// Go: core.go:Memoize/IfElse/OrElse/Coalesce/Identity`
- [x] 序列：`filter_seq / concatenate_seq / enumerate`　`// Go: core.go:FilterSeq/ConcatenateSeq/Enumerate`
- [x] 文本/编码：`compute_ecma_line_starts(_seq) / position_to_line_and_byte_offset / utf16_len(UTF16Offset)`　`// Go: core.go:ComputeECMALineStarts/PositionToLineAndByteOffset/UTF16Len`
- [x] 其它：`must / first_result / stringify_json / get_script_kind_from_file_name / check_each_defined / index_after / should_rewrite_module_specifier / single_element_slice / compare_booleans / apply_debug_stack_limit`　`// Go: core.go:Must/FirstResult/StringifyJson/GetScriptKindFromFileName/CheckEachDefined/IndexAfter/ShouldRewriteModuleSpecifier/SingleElementSlice/CompareBooleans/ApplyDebugStackLimit`
- [x] 拼写建议：`get_spelling_suggestion / get_spelling_suggestion_for_strings` + `levenshtein_with_max`（带 max 剪枝；buffer 复用）　`// Go: core.go:GetSpellingSuggestion/GetSpellingSuggestionForStrings/levenshteinWithMax`

### `text.rs` / `textchange.rs`

- [x] `TextPos(i32)`；`TextRange{pos,end}` + `new/undefined/pos/end/len/is_valid/contains/contains_inclusive/contains_exclusive/with_pos/with_end/contained_by/overlaps/intersects` + `compare_text_ranges`　`// Go: text.go:*`
- [x] `TextChange{range, new_text}` + `apply_to` / `apply_bulk_edits`　`// Go: textchange.go:TextChange/ApplyTo/ApplyBulkEdits`

### `tristate.rs`

- [x] `enum Tristate{Unknown,False,True}` + `is_true/is_true_or_unknown/is_false/is_false_or_unknown/is_unknown/default_if_unknown` + `bool_to_tristate` + serde（true/false/null）+ `Display`（Stringer：`TSUnknown/TSFalse/TSTrue`）　`// Go: tristate.go:*` + `tristate_stringer_generated.go`

### `compileroptions.rs`（含枚举 + Stringer）

- [x] `struct CompilerOptions{...120 字段...}` + `#[derive(Clone,Default)]` + serde（`omitzero`→`skip_serializing_if`）；`EMPTY_COMPILER_OPTIONS`　`// Go: compileroptions.go:CompilerOptions/EmptyCompilerOptions`
- [x] `clone`（derive 替代 reflect）　`// Go: compileroptions.go:(*CompilerOptions).Clone`
- [x] getter：`get_emit_script_target / get_emit_module_kind / get_module_resolution_kind / get_emit_module_detection_kind / get_resolve_package_json_exports / ..._imports / get_allow_importing_ts_extensions / allow_importing_ts_extensions_from / get_resolve_json_module / should_preserve_const_enums / get_allow_js / get_jsx_transform_enabled / get_strict_option_value / get_effective_type_roots / uses_wildcard_types / get_isolated_modules / is_incremental / get_emit_standard_class_fields / get_use_define_for_class_fields / get_emit_declarations / get_are_declaration_maps_enabled / has_json_module_emit_enabled / get_paths_base_path`　`// Go: compileroptions.go:(*CompilerOptions).*`
- [x] 枚举：`ModuleDetectionKind / ModuleKind(含 IsNonNodeESM/SupportsImportAttributes) / ResolutionMode(别名) / ModuleResolutionKind(自定义 Display，Unknown panic) / NewLineKind(GetNewLineKind/GetNewLineCharacter) / ScriptTarget / JsxEmit(自定义 Display)`，保留显式判别值　`// Go: compileroptions.go:*` + `modulekind_stringer_generated.go` + `scripttarget_stringer_generated.go`
- [x] `MODULE_KIND_TO_MODULE_RESOLUTION_KIND` 映射表　`// Go: compileroptions.go:ModuleKindToModuleResolutionKind`

### 其它选项/枚举

- [x] `BuildOptions`（buildoptions.rs）　`// Go: buildoptions.go:BuildOptions`
- [x] `ParsedOptions`（parsedoptions.rs）　`// Go: parsedoptions.go:ParsedOptions`
- [x] `WatchOptions` + `WatchFileKind/WatchDirectoryKind/PollingKind` + `watch_interval`　`// Go: watchoptions.go:*`
- [x] `TypeAcquisition` + `equals`　`// Go: typeacquisition.go:TypeAcquisition/Equals`
- [x] `ProjectReference` + `resolve_project_reference_path / resolve_config_file_name_of_project_reference`　`// Go: projectreference.go:*`
- [x] `ScriptKind` 枚举 + Stringer　`// Go: scriptkind.go` + `scriptkind_stringer_generated.go`
- [x] `LanguageVariant` 枚举 + Stringer　`// Go: languagevariant.go` + `languagevariant_stringer_generated.go`

### 数据结构

- [x] `Arena<T>`（New/NewSlice/NewSlice1/Clone + `next_arena_size`）→ typed-arena/索引方案　`// Go: arena.go:*`
- [x] `LinkStore<K,V>`（get/has/try_get，懒分配）　`// Go: linkstore.go:*`
- [x] `Stack<T>`（push/pop/peek/len，空 pop/peek panic）　`// Go: stack.go:*`
- [x] `Pattern{text, star_index}` + `try_parse_pattern / is_valid / matches / matched_text / find_best_pattern_match`　`// Go: pattern.go:*`
- [x] `binary_search_unique_func`　`// Go: binarysearch.go:BinarySearchUniqueFunc`

### 并发 / BFS

- [x] `Semaphore` trait + `UnlimitedSemaphore` + `LimitedSemaphore`（acquire 返回 RAII release；try_acquire 带取消）　`// Go: semaphore.go:*`
- [x] `WorkGroup` trait + 并行/单线程实现 + `ThrottleGroup`（限流 + 首错收集）　`// Go: workgroup.go:*`
- [x] `BreadthFirstSearchResult/Level/Options` + `breadth_first_search_parallel(_ex)` + `update_min`（原子 CAS）　`// Go: bfs.go:*`
- [x] request id：`with_request_id / get_request_id` 改显式传参（不用 context）　`// Go: context.go:*`

### 杂项

- [x] `version`/`version_major_minor`（`LazyLock`，可被构建期覆盖）　`// Go: version.go:*`
- [x] Node 内置模块：`UNPREFIXED_NODE_CORE_MODULES`/`EXCLUSIVELY_PREFIXED_NODE_CORE_MODULES`/`node_core_modules()`(LazyLock)/`non_relative_module_name_for_typing_cache`　`// Go: nodemodules.go:*`

### Cargo / crate 接线

- [x] `internal/core/Cargo.toml`（`name = "tsgo_core"`，path deps：collections/stringutil/tspath/debug/json + rayon/crossbeam-channel/arena/serde/rustc_hash）
- [x] 根 `Cargo.toml` workspace members 追加
- [x] `lib.rs` 声明全部子模块 + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `text.rs`（TextPos/TextRange）+ `tristate.rs`（含 JSON）——纯值类型，无依赖，tracer bullet。
2. `core.go` 工具族（先 `Filter/Map/Some/Every/Find/Concatenate/Deduplicate/DiffMaps`，纯函数易测）。
3. `compute_ecma_line_starts` / `utf16_len` / `apply_bulk_edits`（文本路径，被 scanner/printer 用）。
4. 数据结构 `Stack/Pattern/binary_search_unique_func/Arena/LinkStore`。
5. 枚举 + `CompilerOptions` 全 getter（逻辑分支多，需细测 `GetEmitModuleKind`/`GetModuleResolutionKind` 等）。
6. `get_spelling_suggestion`（Levenshtein，行为级测试）。
7. 并发：`Semaphore/WorkGroup/ThrottleGroup`，最后 `breadth_first_search_parallel`（**唯一有 Go 单测**，`TestBreadthFirstSearchParallel` 收口 gate；保确定性断言）。

## 与 Go 的已知偏离（divergence）

- **Arena 裸指针 → arena 索引/借用**：见所有权图。`Arena.New()` 的"满了分新 backing"语义在 Rust 用 `typed-arena`（稳定 `&T`）或索引化替代，零 unsafe。
- **reflect Clone → derive(Clone)**：`CompilerOptions.Clone` 用 reflect 遍历导出字段；Rust `#[derive(Clone)]` 直接，行为等价（shallow 复制 + `Rc`/`Vec` 字段克隆）。`noCopy` 标记无需对应。
- **context.Context → 显式传参**：`WithRequestID`/`GetRequestID` 不复刻 context 携带，改为把 `request_id` 显式下传（PORTING §3）。
- **goroutine/atomic 并行 BFS**：用 rayon/scoped threads + 原子；**输出必须确定性**（OrderedMap/IndexMap 保序 + 原子 min 选最低索引），对齐 `TestBreadthFirstSearchParallel` 的固定 Path 断言（如 `[D,B,A]`）。注意测试里 `L2C` 注明"非确定"，Rust 侧也不对其断言。
- **零值返回 → Option**：`FirstOrNil`/`Find` 等返回 `*new(T)`；Rust 地道写法是 `Option<T>`，但为 1:1 可用 `T: Default`。建议公开 `_or_default` + `Option` 版双轨，调用点按需。
- **枚举显式判别值**：`ScriptTarget`/`ModuleKind` 数值不连续（`ESNext=99`/`Node16=100`），`#[repr(i32)]` 必须保留显式值，范围比较（`>= ES2022`）依赖之。
- **sync.Pool（Levenshtein buffer）→ thread_local**：性能优化，Rust 用 `thread_local!` 缓冲或直接分配（`// PERF(port)`）。
- **`ModuleResolutionKind.String()`/`JsxEmit.String()` panic on zero**：保留"零值 panic"语义（移植 bug 探测）。

## 转交 / 推迟（DEFER）

- `CompilerOptions` 的真实解析/验证（命令行/tsconfig）在 P6 `tsoptions`；本包只定义结构 + getter。
- `WorkGroup`/`ThrottleGroup`/BFS 的真实并行用法在 checker/program（P4/P6）；本包实现 + 用 `TestBreadthFirstSearchParallel` 收口。
- `ScriptKind`/`LanguageVariant` 与 AST/scanner 的联动在 P2/P3。
