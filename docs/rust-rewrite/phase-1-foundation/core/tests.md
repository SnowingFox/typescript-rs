# core: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 文件 / 1 顶层 `func Test`（`TestBreadthFirstSearchParallel`，含嵌套 `t.Run` 共 5 个叶子子用例）。

> ⚠️ `core` 28 个实现文件里**只有 `bfs.go` 有直接单测**（`bfs_test.go`）。其余 ~70 个工具函数、文本模型、选项/枚举、数据结构、并发原语**无直接单测**，行为由 **P10 conformance/fourslash parity** 兜底。本轮按 PORTING §8 为关键路径补行为级 Rust 测试。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/core/bfs_test.go` | `internal/core/bfs.rs`（`#[cfg(test)] mod tests`） | 1（嵌套 5 叶子） |

## `bfs_test.go`

> `TestBreadthFirstSearchParallel` 用嵌套 `t.Run` 组织。逐叶子子用例列。图与 `visit` 回调见 input。`assertEqualNumber` 不适用；这里断言 `Stopped`/`Path`/`visited` 集合。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `bfs_find_specific_node` | 找到目标即停，回溯路径 | 图 `A→{B,C},B→{D},C→{D},D→{}`，visit `node=="D"`(stop) → `Stopped=true`、`Path=["D","B","A"]` | `bfs_test.go:TestBreadthFirstSearchParallel/basic functionality/find specific node` | |
| `bfs_visit_all_nodes` | 从不停则遍历全部一次、返回空路径 | 同图，visit 恒 `(false,false)` → `Stopped=false`、`Path==nil`；访问集排序后 `["A","B","C","D"]` | `.../basic functionality/visit all nodes` | |
| `bfs_early_termination` | 目标层以下不访问 | 图 `Root→{L1A,L1B},L1A→{L2A,L2B},L1B→{L2C},L2A→{L3A},...`，visit `node=="L2B"`(stop) → visited 含 Root/L1A/L1B/L2A/L2B、**不含** L3A（L2C 非确定，不断言） | `.../early termination` | |
| `bfs_returns_fallback` | 记录 fallback 但不停 | 同 ABCD 图，visit `node=="A"`(isResult=true, stop=false) → `Stopped=false`、`Path=["A"]`；visited 含 B/C/D | `.../returns fallback when no other result found` | |
| `bfs_stop_over_fallback` | stop 结果优先于 fallback | 同 ABCD 图，visit A→(true,false) 记 fallback、D→(true,true) 停 → `Stopped=true`、`Path=["D","B","A"]` | `.../returns a stop result over a fallback` | |

## 0 直接单测的情况（补充行为级 Rust 测试）

`core.go`/`text.go`/`textchange.go`/`tristate.go`/`compileroptions.go`/数据结构 等 Go 侧无直接单测，行为由 **P10 parity** 兜底。本轮补行为级测试（expected 取自 Go 实现逻辑 / TS 已知行为）：

### 通用工具（core.go）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `filter_keeps_original_when_all_pass` | 全通过返回原切片 | `filter([1,2,3], >0)` → `[1,2,3]` | core.go:Filter | |
| `filter_drops_failing` | 过滤不通过项 | `filter([1,2,3,4], even)` → `[2,4]` | core.go:Filter | |
| `map_basic_and_nil` | 映射；nil 输入返回 nil | `map([1,2],*2)`→`[2,4]`；`map(nil)`→nil/空 | core.go:Map | |
| `some_every` | 存在/全部 | `some([1,2],>1)`→true；`every([1,2],>0)`→true、`>1`→false | core.go:Some/Every | |
| `find_first_last_index` | 查找 | `find([1,2,3],>1)`→2；`find_last`→3；`find_index`→1；`find_last_index`→2 | core.go:Find/FindLast/FindIndex/FindLastIndex | |
| `concatenate_empty_shortcut` | 空捷径 | `concat([1],[])`→`[1]`、`concat([],[2])`→`[2]` | core.go:Concatenate | |
| `splice_basic` | 拼接删除插入 | `splice([1,2,3],1,1,[9])`→`[1,9,3]`；负 start 处理 | core.go:Splice | |
| `deduplicate` | 去重保序 | `deduplicate([1,2,1,3])`→`[1,2,3]` | core.go:Deduplicate | |
| `dedup_sorted` | 有序去重 | `dedup_sorted([1,1,2,3,3], eq)`→`[1,2,3]` | core.go:DeduplicateSorted | |
| `unordered_equal` | 无序相等 | `([1,2,2],[2,1,2])`→true、`([1,2],[1,1])`→false | core.go:UnorderedEqual | |
| `diff_maps_callbacks` | 增/删/改回调 | m1`{a:1,b:2}` m2`{a:1,c:3}` → added c、removed b、changed 无 | core.go:DiffMaps | |
| `min_all_func` | 全部最小元素 | `min_all([3,1,1,2], cmp)`→`[1,1]` | core.go:MinAllFunc | |
| `if_else_or_else_coalesce` | 三元/默认/合并 | `if_else(true,1,2)`→1；`or_else(0,5)`→5、`or_else(3,5)`→3；`coalesce(None,Some)`→Some | core.go:IfElse/OrElse/Coalesce | |
| `memoize_calls_once` | 记忆化只算一次 | `memoize(||{cnt+=1;cnt})` 多次调用都返回 1 | core.go:Memoize | |
| `compute_ecma_line_starts` | 行起点（含 CRLF/LS/PS） | `"a\r\nb\nc"` → `[0,3,5]`（行起点） | core.go:ComputeECMALineStarts | |
| `position_to_line_and_byte_offset` | 位置→行/列 | lineStarts `[0,3,5]`, pos 4 → (line 1, offset 1) | core.go:PositionToLineAndByteOffset | |
| `utf16_len` | UTF-16 长度 | `"abc"`→3；含星体字符（如 `"𝟙"`）→2 | core.go:UTF16Len | |
| `get_script_kind_from_file_name` | 扩展名→ScriptKind | `"a.ts"`→TS、`"a.tsx"`→TSX、`"a.json"`→JSON、`"a.xyz"`→Unknown | core.go:GetScriptKindFromFileName | |
| `compare_booleans` | true>false | `(true,false)`→1、`(false,true)`→-1、`(true,true)`→0 | core.go:CompareBooleans | |
| `spelling_suggestion_close` | 近似拼写建议 | candidates `["foo","bar"]`, name `"fooo"` → `"foo"`；差太远→零值 | core.go:GetSpellingSuggestion | |

### 文本模型（text.go/textchange.go）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `text_range_contains` | 包含判定 | `[0,5)`.contains(0)→t、(5)→f；contains_inclusive(5)→t；contains_exclusive(0)→f | text.go:Contains/ContainsInclusive/ContainsExclusive | |
| `text_range_overlaps_intersects` | 重叠/相交 | `[0,5)`.overlaps(`[5,10)`)→f、intersects→t | text.go:Overlaps/Intersects | |
| `text_range_undefined` | 未定义范围 | `undefined()` → (pos=-1,end=-1)、is_valid 视实现 | text.go:UndefinedTextRange/IsValid | |
| `compare_text_ranges` | 范围比较 | `[0,5)` vs `[0,6)` → 负 | text.go:CompareTextRanges | |
| `apply_to_single` | 单次文本替换 | `TextChange{[1,3),"X"}.apply_to("abcd")` → `"aXd"` | textchange.go:ApplyTo | |
| `apply_bulk_edits` | 批量编辑 | text+多个 edit → 拼接结果 | textchange.go:ApplyBulkEdits | |

### Tristate

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `tristate_predicates` | 谓词 | True.is_true→t；Unknown.is_true_or_unknown→t；False.is_false→t | tristate.go:* | |
| `tristate_json` | JSON 编解码 | True↔`true`、False↔`false`、Unknown↔`null` | tristate.go:MarshalJSON/UnmarshalJSON | |
| `tristate_default_if_unknown` | 缺省 | Unknown.default_if_unknown(True)→True；False.default_if_unknown(True)→False | tristate.go:DefaultIfUnknown | |
| `bool_to_tristate` | 布尔转换 | true→True、false→False | tristate.go:BoolToTristate | |
| `tristate_display` | Stringer | Unknown→`"TSUnknown"`、False→`"TSFalse"`、True→`"TSTrue"` | tristate_stringer_generated.go | |

### CompilerOptions getter（关键分支）

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `get_emit_script_target_default` | Target 缺省 | Target=None → LatestStandard(ES2025) | compileroptions.go:GetEmitScriptTarget | |
| `get_emit_module_kind_by_target` | 由 target 推断 module | Module=None,Target=ESNext→ESNext；Target=ES2022→ES2022；ES2015→ES2015；其它→CommonJS | compileroptions.go:GetEmitModuleKind | |
| `get_module_resolution_kind` | 解析模式推断 | ModuleResolution=Unknown,module=Node16→Node16；NodeNext→NodeNext；其它→Bundler | compileroptions.go:GetModuleResolutionKind | |
| `get_strict_option_value` | strict 派生 | value=Unknown,Strict=True→true；Strict=False→false；value=True→true | compileroptions.go:GetStrictOptionValue | |
| `get_isolated_modules` | 隔离模块 | IsolatedModules=True→t；VerbatimModuleSyntax=True→t | compileroptions.go:GetIsolatedModules | |
| `clone_independent` | Clone 独立 | clone 修改不影响原（Vec/Map 字段） | compileroptions.go:Clone | |
| `module_kind_is_non_node_esm` | ESM 判定 | ES2015..ESNext→t；Node16→f | compileroptions.go:ModuleKind.IsNonNodeESM | |
| `new_line_kind_char` | 换行字符 | CRLF→`"\r\n"`、LF/None→`"\n"`；`get_new_line_kind("\r\n")`→CRLF | compileroptions.go:GetNewLineCharacter/GetNewLineKind | |
| `module_resolution_string_panic_on_unknown` | 零值 panic | `ModuleResolutionKind::Unknown.to_string()` panic | compileroptions.go:ModuleResolutionKind.String | |

### 数据结构 / 杂项

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `stack_push_pop_peek` | 栈操作 | push 1,2 → peek 2、pop 2、pop 1、len 0；空 pop panic | stack.go:* | |
| `pattern_exact_and_star` | 通配匹配 | `try_parse_pattern("a*c")`→star_index 1；matches("abc")→t；matched_text("abc")→"b" | pattern.go:* | |
| `pattern_multiple_stars_invalid` | 多 `*` 无效 | `try_parse_pattern("a*b*c")` → 空 Pattern（star_index 0 text "") | pattern.go:TryParsePattern | |
| `binary_search_unique` | 唯一二分 | 在有序切片找命中→`(idx,true)`、未命中→`(insert,false)` | binarysearch.go:BinarySearchUniqueFunc | |
| `arena_new_stable` | arena 分配稳定引用 | 连续 New 多个，引用都有效且独立 | arena.go:New/NewSlice | |
| `link_store_lazy` | 懒分配链接 | get 不存在的 key→新建并缓存；try_get 不存在→None | linkstore.go:Get/Has/TryGet | |
| `type_acquisition_equals` | 相等比较 | 同字段→t；不同 Include→f；nil 对比 | typeacquisition.go:Equals | |
| `project_reference_resolve` | 解析配置名 | `.json` 路径原样；目录→拼 `tsconfig.json` | projectreference.go:ResolveConfigFileNameOfProjectReference | |
| `node_core_modules` | Node 内置模块表 | `node_core_modules()["fs"]`→t、`["node:fs"]`→t、`["node:sqlite"]`→t；`non_relative_..("fs")`→"node" | nodemodules.go:* | |
| `version_major_minor` | 版本主次号 | `version_major_minor()` 为 `version` 的 `主.次` 前缀 | version.go:VersionMajorMinor | |
| `watch_interval_default` | 默认监听间隔 | nil/无 Interval → 2000ms；有 Interval → 对应 ms | watchoptions.go:WatchInterval | |
| `semaphore_unlimited_limited` | 信号量 | Unlimited.acquire 立即返回；Limited(1) 第二次 try_acquire 在取消时失败 | semaphore.go:* | |
| `workgroup_runs_all` | 工作组执行全部 | 单线程/并行实现都执行所有 Queue 的 fn；RunAndWait 后再 Queue panic | workgroup.go:* | |

## 与 impl.md 的对齐核对

- [x] 唯一的 Go `func Test*`（`TestBreadthFirstSearchParallel`）的 5 个叶子子用例全部映射
- [x] expected 取自 Go 测试字面量（Path `["D","B","A"]`、visited 集合断言、非确定项不断言）
- [x] 每条带 `// Go:` 锚点
- [x] 补充行为级测试覆盖 core.go/text/tristate/compileroptions/数据结构/并发，均在 impl.md 有 TODO 承载
- [x] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `CompilerOptions` 解析/验证端到端 | 需 tsoptions（P6） | P6 / P10 |
| `WorkGroup`/`ThrottleGroup` 真并行竞争正确性 | 需上层并行调用 + 压测 | P4/P6 + 实现期 |
| `get_spelling_suggestion` 与 tsc 诊断建议逐用例一致 | 需 checker 诊断语料 | P10 parity |
| `Arena`/`LinkStore` 在 AST/symbol 图的真实用法 | 需 ast/binder/checker | P2/P3/P4 |
| `compute_ecma_line_starts`/`utf16_len` 在 scanner/LSP 的端到端 | 需 scanner/lsp | P3/P8 + P10 |
