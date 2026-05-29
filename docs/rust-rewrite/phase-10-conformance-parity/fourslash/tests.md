# fourslash: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应测试且 `cargo test` 通过；留空=未写/未过；`—`=推迟/分批中。
**Go 测试规模**：**4250 个 `*_test.go` / 4386 个 `func Test*`**（`tests/` 277 文件 / 413 func + `tests/gen/` 3768 / 3768 + `tests/manual/` 205 / 205）。

> ⚠️ **本文件不逐个枚举 4250 个测试。** PORTING §8 的"逐 func / 逐子用例对齐"对**框架代码**（6 个核心 `.go` 的 `Verify*` 命令）适用；但对 4250 个**生成测试**，正确口径是**parity 批次门控**——它们由生成器从同一批 TS fixture 产出，"通过"= 跑出的 baseline / 内联断言与 Go 一致。逐条枚举 4250 行既不可维护也违背"复用 fixture 不翻译"原则（见 [impl.md 复用 fixture 的生成器思路](./impl.md)）。

## 测试分两层

| 层 | 测哪 | 对齐口径 |
|---|---|---|
| **L1 框架自测** | 6 个核心 `.go`（`test_parser` DSL / baseline 渲染 / `tests/util` 常量 / `Verify*` 命令语义） | 逐函数行为级测试（本文件 §1，expected 取自 Go 实测 / reference） |
| **L2 parity 批次** | 4250 个生成测试 | 按命令族 + 按 baseline 对拍分批门控（本文件 §2，checklist 形态，不逐条列） |

## §1 L1 框架自测（逐函数，必须逐条对齐）

### §1.1 `test_parser` — DSL 解析（fourslash 的命门，最需精确）

> Go 无 `test_parser_test.go`（0 直接单测）；但 DSL 解析错一处，4250 个测试全错。依据 PORTING §8.5 **必须补**行为级测试，expected 由对 `parseFileContent` 状态机的精确推演给出。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parse_named_marker` | `/*x*/` 抠成 marker，内容净化 | `"a/*x*/b"` → content `"ab"`, marker{name:"x", position:1} | `parseFileContent`（slash-star 分支） | |
| `parse_anonymous_marker_empty` | `/**/` 匿名 marker | `"a/**/b"` → content `"ab"`, marker name="" position:1 | 同上 | |
| `parse_range` | `[\|text\|]` 抠成 range，内容保留 text | `"x[\|foo\|]y"` → content `"xfooy"`, range{pos:1,end:4} | `parseFileContent`（range 分支） | |
| `parse_range_with_marker` | range 内嵌 marker | `"[\|/*m*/foo\|]"` → content `"foo"`, range.marker.name=="m" | `parseFileContent`（openRanges[last].marker） | |
| `parse_object_marker_json` | `{\| "name":"n","x":1 \|}` JSON 解析 | → marker{name:"n", data:{x:1}} | `getObjectMarker` | |
| `parse_object_marker_anonymous` | 无 name 的对象 marker | `{\| "x":1 \|}` → marker name=None, data 非空 | `getObjectMarker` | |
| `parse_block_comment_not_marker` | 含非法字符的 `/* ... */` 当普通块注释回吐 | `"/* a b */"` → 内容保留，无 marker | `parseFileContent`（invalid marker char 分支） | |
| `parse_multifile_filename` | `// @Filename:` 切多文件（复用 testrunner） | 2 个 `@Filename` → 2 files | `ParseTestData` → testrunner | |
| `parse_duplicate_marker_name_fatal` | 同名 marker 报错 | 两个 `/*x*/` → fatal "Duplicate marker name" | `ParseTestData` 行 ~149 | |
| `parse_unterminated_range_error` | `[\|` 未闭合报错 | `"a[\|b"` → "Unterminated range" | `parseFileContent` 行 ~407 | |
| `parse_chomp_leading_space` | 行首统一空格被 chomp | 每行首空格 → 去一格 | `chompLeadingSpace` | |
| `parse_ls_position_utf8` | marker 的 LSPosition 按 UTF-8 算 line/char | 跨行 marker → 正确 (line, char) | `ParseTestData` 尾段（converters） | |

### §1.2 `tests/util` — completion 常量 + 排序

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `default_commit_characters` | 默认提交字符 | → `[".", ",", ";"]` | `util.go:DefaultCommitCharacters` | |
| `completion_globals_sorted` | `CompletionGlobals` 按 sortText 再 label（大小写不敏感优先）排序 | 排序稳定、与 Go 一致 | `util.go:sortCompletionItems` | |
| `completion_globals_plus_nolib` | `CompletionGlobalsPlus(items, noLib=true)` 不含 global vars | noLib → 仅 keywords + this/undefined + items | `util.go:CompletionGlobalsPlus` | |
| `in_js_keywords_filtered` | `getInJSKeywords` 过滤掉 type-only 关键字 | 去掉 enum/interface/type… | `util.go:getInJSKeywords` | |

### §1.3 `baselineutil` — 命令→文件名/扩展名/选项

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `baseline_extension_jsonc_default` | 默认命令扩展名 | `goToDefinition` → `.baseline.jsonc` | `getBaselineExtension` default | |
| `baseline_extension_special` | 特例扩展名 | `QuickInfo`/`SignatureHelp`/… → `.baseline`；`Call Hierarchy` → `.callHierarchy.txt`；`Auto Imports` → `.baseline.md`；`linkedEditing` → `.linkedEditing.txt` | `getBaselineExtension` switch | |
| `baseline_filename_from_test` | 测试名反推文件名 | `TestConstructorFindAllReferences1` → `constructorFindAllReferences1` | `getBaseFileNameFromTest` | |
| `baseline_filename_callhierarchy_special` | callHierarchyFunctionAmbiguityN 特例 | `...Ambiguity1` → `callHierarchyFunctionAmbiguity.1` | `getBaseFileNameFromTest` switch | |
| `baseline_subfolder` | 子目录 = `fourslash/<cmd>` | `findAllReferences` → `fourslash/findAllReferences` | `getBaselineOptions` | |
| `add_result_separator` | 多命令结果间 `\n\n\n\n` + `// === <cmd> ===` 头 | 两次 add → 含分隔 | `addResultToBaseline` | |

### §1.4 `Verify*` / `GoTo*` 命令语义（抽样关键命令，行为级）

> 不为每个 `Verify*` 写孤立单测（它们的正确性由 L2 parity 兜底）；只对**有独立可测逻辑**的命令补行为测试。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `goto_marker_moves_caret` | `GoToMarker("x")` 把光标移到 marker | → currentCaretPosition == marker.LSPosition | `fourslash.go:GoToMarker` | |
| `verify_completions_includes` | `Includes` 子集匹配（不要求 exact） | completion 含 "T" → pass；不含 → fail | `fourslash.go:VerifyCompletions` | |
| `verify_completions_excludes` | `Excludes` 反向断言 | 含被排除项 → fail | `fourslash.go:VerifyCompletions` | |
| `insert_then_current_content` | `Insert` + `VerifyCurrentFileContent` | 插入后内容正确（scriptInfo 更新 + didChange） | `fourslash.go:Insert` / `VerifyCurrentFileContent` | |
| `skip_if_failing_skips_listed` | `failingTests.txt` 内的测试被 skip | 名单内 → Skipped；`TSGO_FOURSLASH_IGNORE_FAILING` → 不 skip | `skip_if_failing.go:SkipIfFailing` | |

## §2 L2 parity 批次门控（4250 测试，checklist 形态）

> 这是 4250 测试的"完成"口径：**不逐条列**，按命令族分批，每批门控 = "该族生成的测试全部跑通且 baseline/内联断言与 Go 一致（或入 `failingTests.txt`）"。

### §2.1 命令族分批（按 baseline 命令 / 内联命令聚类）

| 批次 | 命令族（对应 `VerifyBaseline*` / `Verify*`） | reference 子目录 / 形态 | 代表测试（抽样，非全列） | 完成 |
|---|---|---|---|---|
| B1 | completion（内联 `VerifyCompletions`） | 内联断言（无 baseline 文件） | `asOperatorCompletion` / `argumentCompletions` / `autoImportCompletion` | — |
| B2 | go-to-definition / type / source / implementation | `fourslash/goToDefinition/*.baseline.jsonc` 等 | `ambientShorthandGotoDefinition` | — |
| B3 | find-all-references / vsFindAllReferences | `fourslash/findAllReferences/*.baseline.jsonc` | `constructorFindAllReferences1` / `ambientShorthandFindAllRefs` | — |
| B4 | quickinfo / hover（内联 + baseline） | `fourslash/QuickInfo/*.baseline` | （hover 族） | — |
| B5 | signature help | `fourslash/SignatureHelp/*.baseline` | （signature 族） | — |
| B6 | rename / willRenameFiles | `fourslash/findRenameLocations/*.baseline.jsonc` | （rename 族；注意 submodule preference fixup） | — |
| B7 | document highlights / document symbol / folding / outlining | 各对应子目录 | — | — |
| B8 | inlay hints / linked editing / selection ranges / call hierarchy / code lens | 各对应子目录（`.callHierarchy.txt` 等） | — | — |
| B9 | diagnostics（baseline + 内联） | `fourslash/Syntax and Semantic Diagnostics/*.baseline` | — | — |
| B10 | code fix / organize imports / auto imports（仅 allowed fixId 子集） | `fourslash/Auto Imports/*.baseline.md` 等 | — | — |
| B11 | `tests/manual/`（205 手工） | 混合 | （手写路径，方案 C） | — |
| B12 | `@statebaseline` 模式 | `fourslash/state/*.baseline` | — | — |

### §2.2 每批的门控标准（red→green 纪律）

每批进入 `✓` 需满足：

1. 该族**生成器输出**（方案 A：改 `convertFourslash.mts` 模板）能产出 Rust 测试，`cargo build` 通过。
2. 跑该批：通过的从 `_scripts/failingTests.txt` 移除；未通过的留在名单（**skip 不算红**，是受控未完成）。
3. **baseline 类测试**：跑出的 baseline 与 `testdata/baselines/reference/fourslash/<cmd>/<name>.<ext>` **逐字节一致**（`baseline::run` 不报 changed）。
4. **内联类测试**（completion/quickinfo 等）：内联 expected（来自 `tests/util` 常量 + 生成器搬运的字面量）断言相等。
5. 失败路径覆盖：每批含 negative 断言（`Excludes` / `VerifyNoErrors` / `VerifyCodeFixNotAvailable` 等）至少 1 例。

### §2.3 进度统计口径（替代逐条勾选）

> 用"通过数 / 总数 / skip 数"汇总，而非 4250 行表格。例：

| 指标 | 目标 | 当前 |
|---|---|---|
| 生成的 Rust 测试文件数 | == 4250 - `unparsedTests.txt`(2603) 的可转换部分（与 Go `tests/gen` 数对齐 ≈ 3768） | — |
| `cargo test -p tsgo_fourslash` 通过数 | 随批次递增 | — |
| `failingTests.txt` 规模 | 随批次递减（初始可接近全量） | — |
| baseline 逐字节一致率（按命令族） | 每族 100% 才该族 `✓` | — |

## 与 impl.md 的对齐核对

- [ ] L1：`test_parser` 全部 DSL 分支（marker/range/object/multifile/error）有测试行 ↔ impl.md `parse_file_content` TODO
- [ ] L1：`tests/util` 常量 + 排序 ↔ impl.md `tests/util/mod.rs` TODO
- [ ] L1：`baselineutil` 文件名/扩展名/子目录 ↔ impl.md `baselineutil.rs` TODO
- [ ] L1：关键 `Verify*`/`GoTo*` 语义 ↔ impl.md `lib.rs` 命令 TODO
- [ ] L2：4250 测试用 parity 批次门控（B1-B12）覆盖，不逐条；每批映射到 impl.md 的命令族 + 生成器思路
- [ ] expected 取自 Go 实测 / `reference/fourslash/**` 真实 baseline / `tests/util` 常量（非 Rust 推断）
- [ ] 每条 L1 测试带 `// Go:` 锚点
- [ ] 说明了"不逐条枚举 4250"的理由（生成、复用 fixture）

## 推迟到后续 phase / 分批的测试

| 测试 / 行为 | 原因 | 目标 |
|---|---|---|
| B4-B12 各命令族 | 依赖对应 `internal/ls` feature 在 P7 全量落地；按族 red→green | 分批（本 phase 内） |
| `@statebaseline`（B12） | 仅少量测试，靠后 | 主命令族绿后 |
| `unparsedTests.txt`(2603) 对应的 TS 上游用例 | Go 自己也没转换（命令未实现 / 不支持的 code fix）；Rust 同样不转 | 不做（与 Go 对齐） |
| `tests/manual/`(205) 手写路径 | 与生成路径并存，量小靠后 | B11 |
