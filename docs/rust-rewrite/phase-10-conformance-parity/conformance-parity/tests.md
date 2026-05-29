# conformance-parity: 测试清单（tests.md）

**完成列**：`✓`=该格 baseline 在对应子目录全量逐字节一致；留空=未达成；`—`=推迟/分批中。
**Go 测试规模**：无独立 `*_test.go`——conformance-parity 由 `tsgo_testrunner::TestLocal` / `TestSubmodule` 驱动（见 [../testrunner/tests.md](../testrunner/tests.md)）；"测试"= 几万个 baseline 逐字节对拍。

> 这里不逐个列几万个 baseline 文件，而是用 **(baseline 类型 T) × (子目录 D) 矩阵 + 漂移分类** 作门控。每个格子是一个可勾选里程碑。ground truth = `testdata/baselines/reference/**`（292MB）。

## §1 对拍矩阵（T × D 门控）

> 行 = baseline 类型（先决 phase）；列 = fixture 子目录。每格"全量逐字节一致"才 `✓`。

| baseline 类型 \ 子目录 | `compiler/`(227 fixture) | `conformance/`(7 子目录) | submodule(diff) |
|---|---|---|---|
| **T1 `.errors.txt`**（诊断；P2/P3/P4） | — | — | — |
| **T2 `.symbols`**（P3/P4） | — | — | — |
| **T3 `.types`**（P4 checker） | — | — | — |
| **T4 `.js` / `.d.ts`**（P5） | — | — | — |
| **T5 `.js.map` / `.sourcemap.txt`**（P5） | — | — | — |
| **T6 module resolution trace**（P4 module） | — | — | — |

推进顺序：`(T1,compiler) → (T1,conformance) → (T2,compiler) → … → (T6,*) → submodule`。

## §2 结构自检（非 baseline，但 TestLocal 必跑）

| 检查 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `no_duplicate_test_files` | compiler + conformance 两 runner 文件名无重复 | `compiler_runner_test.go:runCompilerTests`（seenTests） | |
| `verify_union_ordering` | 每个 union 的 `CompareTypes` 排序稳定（反转 + 10 次 shuffle 后一致） | `compiler_runner.go:verifyUnionOrdering` | |
| `verify_parent_pointers` | 每个非 lib 源文件 AST 的 parent 指针与遍历父一致 | `compiler_runner.go:verifyParentPointers` | |

> `verify_union_ordering` / `verify_parent_pointers` 是 **AST arena 模型（PORTING §5）正确性的守门**：parent 用 `NodeId` 索引后，这个自检确保 `node.parent(arena)` 全程正确。

## §3 漂移分类（对拍失败时的归因 checklist）

> baseline 不一致时按类归因，避免逐文件盲调。每类是一个修复批次。

| 漂移类别 | 典型表现 | 根因定位 | 修复归属 phase |
|---|---|---|---|
| 诊断消息文本差异 | `.errors.txt` 里 `!!! error TSxxxx:` 后文本不同 | i18n 消息表 / 参数插值 | P2 `diagnostics` |
| 诊断顺序差异 | 错误行顺序不同 | 并行收集后排序不稳定 | 本 phase（确定性重排） |
| 类型显示差异 | `.types` 里 `A \| B` 成员顺序 | union ordering / `CompareTypes` | P4 `checker`（`verify_union_ordering` 会先报） |
| 符号解析差异 | `.symbols` 里 Symbol(...) 声明位置 | binder symbol 表 | P3 `binder` / P4 |
| emit 文本差异 | `.js` 内容/空白/分号 | printer / transformers | P5 |
| emit 顺序差异 | `//// [path]` 块顺序 | 并行 emit 未按 input 重排 | 本 phase（`new_compilation_result`） |
| 换行差异 | 全片 `\n` vs `\r\n` | 默认 NewLine 没设 CRLF | 本 phase（`CompileFiles` 默认） |
| sourcemap 差异 | `.js.map` mappings VLQ | sourcemap 编解码 | P5 `sourcemap` |
| 路径前缀差异 | `==== ./x` vs `==== x` | `DiffFixupOld` 未实现 | 本 phase（`verify_diagnostics`） |
| 版本号差异 | trace 里真实版本 vs FakeTSVersion | `TracerForBaselining` 净化缺失 | 本 phase（testutil） |

## §4 baseline tracking（防腐）

| 检查 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `tracking_records_used_baselines` | 启用 `TSGO_BASELINE_TRACKING_DIR` 后，写过的 baseline 路径被记录 | `testmain.go:Track` / `recordBaseline` | |
| `unused_reference_detection` | reference 下存在但无任何测试写过的 baseline 被标记（防腐烂） | Go tracking 工具链 | — |

## 与 impl.md 的对齐核对

- [ ] §2/§4 结构自检每条带 `// Go:` 锚（「Go 对照」列指向 Go runner/testmain 源 `internal/testrunner/compiler_runner.go:<Func>`）

- [ ] §1 矩阵每行对应 impl.md 的一个 baseline 类型 TODO（T1-T6）
- [ ] §1 矩阵每列对应 impl.md 分批维度 B（compiler / conformance / submodule）
- [ ] §2 结构自检对应 testrunner `verify_union_ordering` / `verify_parent_pointers`
- [ ] §3 漂移分类把每类失败归到具体 phase（可执行的修复路径）
- [ ] ground truth 明确 = `testdata/baselines/reference/**`（非 Rust 推断）
- [ ] submodule（D3）标 DEFER，依赖 local 全绿 + checkout

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 |
|---|---|---|
| submodule diff（D3 列） | 依赖 `../_submodules/TypeScript` checkout + 三档 diff | local 全绿后 |
| incremental / buildinfo baseline | 依赖 P6 `incremental` | P6 落地后 |
| `captureSuggestions` 建议诊断 baseline | 依赖 checker suggestion 诊断 | P4 全量后 |
| `unused_reference_detection` | tracking 工具链非对拍主路径 | 收尾 |
