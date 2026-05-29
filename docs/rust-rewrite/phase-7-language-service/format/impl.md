# format: 实现方案（impl.md）

> 写之前已实际通读 `internal/format/*.go`（10 个非测试文件）。所有 TODO 带 `// Go:` 锚点。

**crate**：`tsgo_format`　**目标**：把一段 TS/JS 源码（或其中一个区间 `span`）按用户的 `FormatCodeSettings` 重新缩进 / 调整空白 / 增删分号，产出一组 `TextChange`（替换/插入/删除编辑），不改 AST、不重排语句。
**依赖（crate）**：`tsgo_ast` `tsgo_astnav` `tsgo_core` `tsgo_scanner` `tsgo_debug` `tsgo_stringutil` `tsgo_ls_lsutil`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/format/`（10 个非测试文件，约 4.3 万行含规则表；`span.go`/`rules.go`/`indent.go` 最大）

> ⚠️ 循环依赖提示：Go 里 `internal/format` 依赖 `internal/ls/lsutil`（`FormatCodeSettings` / `GetDefaultFormatCodeSettings`），而 `internal/ls/lsutil` 又依赖 `internal/format`（无直接 import，但同 phase）。在 Rust workspace 里 `FormatCodeSettings`/`FormatCodeSettings` 定义在 `tsgo_ls_lsutil`，`tsgo_format` 依赖它；`tsgo_ls_lsutil` **不**依赖 `tsgo_format`（已核对：`lsutil/formatcodeoptions.go` 仅 import `printer`/`lsproto`/`core`，无 `format`），故无环。详见 README「包关系」。

## 这个包是什么（业务说明）

格式化器是语言服务里所有「插入/修改代码」操作的底座：`textDocument/formatting`、`rangeFormatting`、`onTypeFormatting`（敲 `}`/`;`/换行/`{` 时自动格式化），以及 code action / organize imports / rename 在生成新节点文本时都会调它（见 `tsgo_ls` 的 `change.Tracker`）。它**不**做语法/语义分析，只消费已 parse 好的 `ast.SourceFile` + scanner。

核心流程（见 `internal/format/README.md`）：每个公开 `Format*` 入口都收敛到 `FormatSpan` → `newFormattingScanner` + `formatSpanWorker.execute`。`span` 是要格式化的文本区间；worker 从「完整包住该 span 的最高节点」开始，递归下降遍历子节点（`processNode`），对每一对「相邻 token（左/右）」调用 `processPair`，向 `rulesMap` 查一组可应用的规则（`rules.go` 里 ~250 条），每条规则声明左右 token 类型 + 一组上下文谓词（`rulecontext.go`）+ 一个动作（插空格/换行/删空格/删 token/插分号）。命中的规则产出 `TextChange`。缩进由 `indent.go` 的 `dynamicIndenter` 沿递归向下传递。

为什么在 P7：依赖 P2/P3 的 `ast`/`scanner`/`astnav`，被 P7 的 `ls`、P8 的 `lsp` 使用，是语言服务最先能独立 TDD 的叶子之一。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `context.Context` 透传 `FormatCodeSettings`/newline（`api.go` 的 `WithFormatCodeSettings`/`GetFormatCodeSettingsFromContext`） | **显式参数**：把 `&FormatCodeSettings` + `new_line: &str` 直接作为参数传进 `format_span` 等，不复刻 `context.WithValue` 魔法 | Go 用 context 是为了跨语言服务层透传；Rust 侧改显式传参更安全清晰。这是允许的偏离，记 `// DEFER(port)` 不需要 |
| `FormatRequestKind`（iota 6 值） | `#[repr(i32)] enum FormatRequestKind` | document/selection/onEnter/onSemicolon/onOpeningCurly/onClosingCurly |
| `ruleAction`（位运算 const，`1<<n`） | `bitflags! struct RuleAction` | InsertSpace/InsertNewLine/DeleteSpace/DeleteToken/InsertTrailingSemicolon + Stop* 组合掩码 |
| `ruleFlags`（iota） | `#[repr(i32)] enum RuleFlags` | None / CanDeleteNewLines |
| `contextPredicate = func(*FormattingContext) bool` | `type ContextPredicate = fn(&FormattingContext) -> bool`（或 `&'static dyn Fn`） | ~120 个谓词函数（`rulecontext.go`）；规则表里是 `&[ContextPredicate]` |
| `ruleImpl{ debugName, context, action, flags }` | `struct RuleImpl { debug_name: &'static str, context: &'static [ContextPredicate], action: RuleAction, flags: RuleFlags }` | 规则是**全局静态**（`getRulesMap = sync.OnceValue`） → Rust 用 `once_cell::sync::Lazy` / `std::sync::OnceLock` |
| `tokenRange{ tokens []ast.Kind, isSpecific bool }` | `struct TokenRange { tokens: Vec<ast::Kind>, is_specific: bool }` | 规则左右 token 集合 |
| `getRulesMap` 的 `[][]*ruleImpl` bucket（`mapRowLength = LastToken+1`，二维展平 `row*len+col`） | `Vec<Vec<&'static RuleImpl>>`，索引 `getRuleBucketIndex(row,col)` 同算法 | `buildRulesMap` 用位图 `RulesPosition`（6 段 × 5bit）排序插入，**1:1 复刻**（`rulesmap.go`） |
| `core.Tristate`（TSUnknown/TSTrue/TSFalse 缓存 line-on-same-line） | `tsgo_core::Tristate` | `FormattingContext` 里 5 个懒缓存字段 |
| `formattingScanner`（持有 `*scanner.Scanner`，可变游标 + rescan 状态） | `struct FormattingScanner<'a>`，内部 `Scanner` + 状态机；`scanAction` enum | 大量 `ReScan*`（>,/,template,jsx），保持与 scanner 的协议 |
| `TextRangeWithKind{ Loc core.TextRange, Kind ast.Kind }` | `struct TextRangeWithKind { loc: TextRange, kind: ast::Kind }` | token + trivia 的轻量表示 |
| `[]core.TextChange` 返回 | `Vec<tsgo_core::TextChange>` | 编辑结果；`applyBulkEdits` 在测试侧 |

**无 arena 新增**：本包不持有 AST，只读 `&ast::SourceFile` + `NodeId` 访问器（来自 `tsgo_ast`/`tsgo_astnav`）。`node.Parent` → `node.parent(arena)` 同 PORTING §5 全局约定。

**确定性**：格式化输出的 `TextChange` 序列必须与 Go 完全同序（`applyBulkEdits` 依赖编辑按 pos 升序、不重叠）。worker 顺序遍历，无并行点，天然确定。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/format/api.go` | `internal/format/api.rs` | 公开入口：`FormatDocument`/`FormatSelection`/`FormatOnEnter`/`FormatOnSemicolon`/`FormatOnOpeningCurly`/`FormatOnClosingCurly`/`FormatSpan`/`FormatNodeGivenIndentation` + 设置透传 |
| `internal/format/context.go` | `internal/format/context.rs` | `FormattingContext`：当前/下一 token span + parent + contextNode，5 个 same-line 懒缓存（Tristate） |
| `internal/format/rule.go` | `internal/format/rule.rs` | `ruleImpl`/`ruleSpec`/`tokenRange`/`ruleAction`/`ruleFlags`/`rule()` 构造器/`toTokenRange` |
| `internal/format/rulecontext.go` | `internal/format/rulecontext.rs` | ~120 个 `contextPredicate`（`isBinaryOpContext` 等）+ option selector 工具 |
| `internal/format/rules.go` | `internal/format/rules.rs` | `getAllRules()` 返回全部 ~250 条 `ruleSpec`（规则表本体）+ `tokenRangeFrom*` |
| `internal/format/rulesmap.go` | `internal/format/rulesmap.rs` | `buildRulesMap`/`getRules`/`getRuleBucketIndex`/`addRule`（bucket 位图排序）+ `getRuleActionExclusion` |
| `internal/format/scanner.go` | `internal/format/scanner.rs` | `formattingScanner`：trivia 感知扫描 + `ReScan*` 状态机；`tokenInfo`/`TextRangeWithKind`/`scanAction` |
| `internal/format/span.go` | `internal/format/span.rs` | `formatSpanWorker`（execute/processNode/processPair/applyRuleEdits/trim 空白）+ `dynamicIndenter` + `findEnclosingNode` |
| `internal/format/indent.go` | `internal/format/indent.rs` | `GetIndentation`/`GetIndentationForNode`/`GetContainingList`/`ShouldIndentChildNode`/`NodeWillIndentChild` 等缩进算法 |
| `internal/format/util.go` | `internal/format/util.rs` | `GetLineStartPositionForPosition`/`findOutermostNodeWithinListLevel`/`isGrammarError`/`getOpenTokenForList` 等小工具 |
| —（lib 根） | `internal/format/lib.rs` | crate 根：`mod` 声明 + re-export 公开 API（basename `format`==dir 名 → `lib.rs`，见 PORTING §2） |

## 依赖白名单（本包新增的 crate）

- `bitflags`（PORTING §10 已列）：`ruleAction`/`ruleFlags`。
- `once_cell` 或 `std::sync::OnceLock`：复刻 `sync.OnceValue(buildRulesMap)` 的全局懒初始化规则表。**建议执行期 `cargo add once_cell`（若不用 std OnceLock），并记入 crate-map.md「待定」转正**。
- 无其他新增（`rustc_hash`/`indexmap` 本包不直接需要）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按 TDD 推进序：先小工具 + 规则数据结构，再 scanner，再 worker，最后入口。

### `util.rs`（Go: `internal/format/util.go`）

- [ ] `pub fn get_line_start_position_for_position(position: i32, file: &SourceFile) -> i32` — 行首位置　`// Go: util.go:GetLineStartPositionForPosition`
- [ ] `fn range_is_on_one_line(node: TextRange, file: &SourceFile) -> bool`　`// Go: util.go:rangeIsOnOneLine`
- [ ] `fn get_open_token_for_list(node, list) -> ast::Kind` / `fn get_close_token_for_open_token(kind) -> ast::Kind`　`// Go: util.go:getOpenTokenForList/getCloseTokenForOpenToken`
- [ ] `fn is_grammar_error(parent, child) -> bool` + `fn is_grammar_error_element(...)`　`// Go: util.go:isGrammarError`
- [ ] `fn find_immediately_preceding_token_of_kind(end, kind, file) -> Option<NodeId>`　`// Go: util.go:findImmediatelyPrecedingTokenOfKind`
- [ ] `fn find_outermost_node_within_list_level(node) -> NodeId` + `fn is_list_element(parent, node) -> bool`　`// Go: util.go:findOutermostNodeWithinListLevel/isListElement`

### `rule.rs`（Go: `internal/format/rule.go`）

- [ ] `bitflags! pub struct RuleAction`（含 StopAction/ModifySpaceAction/ModifyTokenAction 组合常量）　`// Go: rule.go:ruleAction`
- [ ] `#[repr(i32)] pub enum RuleFlags { None, CanDeleteNewLines }`　`// Go: rule.go:ruleFlags`
- [ ] `pub struct TokenRange { tokens, is_specific }` + `fn to_token_range(...)`（接受 `Kind` / `&[Kind]` / `TokenRange`）　`// Go: rule.go:tokenRange/toTokenRange`
- [ ] `pub struct RuleImpl { debug_name, context, action, flags }` + 访问器 + `Display`(=debugName)　`// Go: rule.go:ruleImpl`
- [ ] `pub struct RuleSpec { left_token_range, right_token_range, rule }` + `fn rule(...) -> RuleSpec`　`// Go: rule.go:rule`
- [ ] `pub type ContextPredicate = fn(&FormattingContext) -> bool`；`pub const ANY_CONTEXT: &[ContextPredicate] = &[]`　`// Go: rule.go:contextPredicate/anyContext`

### `rulecontext.rs`（Go: `internal/format/rulecontext.go`）

- [ ] option selector 工具：`option_equals`/`is_option_enabled`/`is_option_disabled`/`is_option_disabled_or_undefined`/`...or_tokens_on_same_line`/`is_option_enabled_or_undefined`　`// Go: rulecontext.go:isOptionEnabled...`
- [ ] 22 个 `*Option(opts) -> Tristate/SemicolonPreference` 取值器（`semicolonOption`/`insertSpaceAfterCommaDelimiterOption` …）　`// Go: rulecontext.go:semicolonOption...`
- [ ] ~90 个上下文谓词（逐个 1:1）：`isForContext`/`isBinaryOpContext`/`isTypeAnnotationContext`/`isFunctionDeclContext`/`isControlDeclContext`/`isObjectContext`/`isFunctionCallContext`/`isArrowFunctionContext`/`isJsx*Context`/`isSemicolonDeletionContext`/`isSemicolonInsertionContext`/`isNotPropertyAccessOnIntegerLiteral` … 全量见 `rulecontext.go`　`// Go: rulecontext.go:<Func>`

### `rules.rs`（Go: `internal/format/rules.go`）

- [ ] `pub fn get_all_rules() -> Vec<RuleSpec>` — **全部 ~250 条规则逐条复刻**（左右 token + 谓词数组 + 动作 + flag）。是本包最长函数，需逐行核对　`// Go: rules.go:getAllRules`
- [ ] `fn token_range_from(...) -> TokenRange` / `token_range_from_ex(prefix, ...)` / `token_range_from_range(start, end)`　`// Go: rules.go:tokenRangeFrom*`

### `rulesmap.rs`（Go: `internal/format/rulesmap.go`）

- [ ] `static RULES_MAP: OnceLock<Vec<Vec<&'static RuleImpl>>>` + `fn get_rules_map()`（复刻 `sync.OnceValue`）　`// Go: rulesmap.go:getRulesMap`
- [ ] `fn build_rules_map() -> Vec<Vec<&RuleImpl>>`　`// Go: rulesmap.go:buildRulesMap`
- [ ] `fn get_rules(ctx, out) -> Vec<&RuleImpl>`（按 action 掩码过滤 + 谓词短路）　`// Go: rulesmap.go:getRules`
- [ ] `fn get_rule_bucket_index(row, col) -> usize`（断言 ≤ LastKeyword）+ 常量 `MASK_BIT_SIZE/MASK/MAP_ROW_LENGTH`　`// Go: rulesmap.go:getRuleBucketIndex`
- [ ] `fn get_rule_action_exclusion(action) -> RuleAction`　`// Go: rulesmap.go:getRuleActionExclusion`
- [ ] `fn add_rule(...)` + `RulesPosition` enum + `get_rule_insertion_index`/`increase_insertion_index`（5bit×6 位图）　`// Go: rulesmap.go:addRule`

### `context.rs`（Go: `internal/format/context.go`）

- [ ] `pub struct FormattingContext { current_token_span, next_token_span, context_node, current_token_parent, next_token_parent, 5×Tristate cache, source_file, formatting_request_kind, options }`　`// Go: context.go:FormattingContext`
- [ ] `pub fn new(file, kind, options) -> FormattingContext`　`// Go: context.go:NewFormattingContext`
- [ ] `fn update_context(cur, cur_parent, next, next_parent, common_parent)`（panic 三处 nil 检查 + 清缓存）　`// Go: context.go:UpdateContext`
- [ ] `ContextNodeAllOnSameLine`/`NextNodeAllOnSameLine`/`TokensAreOnSameLine`/`ContextNodeBlockIsOnOneLine`/`NextNodeBlockIsOnOneLine`（懒缓存）　`// Go: context.go:*OnSameLine`
- [ ] `fn block_is_on_one_line` / `node_is_on_one_line` / `range_is_on_one_line`　`// Go: context.go:blockIsOnOneLine...`

### `scanner.rs`（Go: `internal/format/scanner.go`）

- [ ] `pub struct TextRangeWithKind` + `new_text_range_with_kind(pos,end,kind)`　`// Go: scanner.go:TextRangeWithKind`
- [ ] `struct FormattingScanner`（持 `Scanner` + leading/trailing trivia + lastTokenInfo + 状态）　`// Go: scanner.go:formattingScanner`
- [ ] `pub fn new_formatting_scanner(text, lang_variant, start, end, worker) -> Vec<TextChange>`（建 scanner→`worker.execute`→reset）　`// Go: scanner.go:newFormattingScanner`
- [ ] `advance` / `read_token_info(n)` / `get_next_token(n, action)` / `read_eof_token_range`　`// Go: scanner.go:advance/readTokenInfo/...`
- [ ] `scanAction` enum + `shouldRescan*`（GreaterThan/Slash/Template/JsxIdentifier/JsxText/JsxAttributeValue）+ `fixTokenKind`　`// Go: scanner.go:shouldRescan*`
- [ ] `is_on_token`/`is_on_eof`/`skip_to_end_of`/`skip_to_start_of`/`get_current_leading_trivia`/`last_trailing_trivia_was_new_line`/`get_token_full_start`　`// Go: scanner.go:isOnToken...`

### `indent.rs`（Go: `internal/format/indent.go`）

- [ ] `pub fn get_indentation_for_node(n, ignore_range, file, options) -> i32`　`// Go: indent.go:GetIndentationForNode`
- [ ] `pub fn get_indentation(position, file, options, assume_newline_before_close_brace) -> i32`（核心：smart/block/none 三模式）　`// Go: indent.go:GetIndentation`
- [ ] `pub fn get_containing_list(node, file) -> Option<&NodeList>`（被 `indent_test.go` 直接测）　`// Go: indent.go:GetContainingList`
- [ ] `pub fn should_indent_child_node(...)` / `pub fn node_will_indent_child(...)`　`// Go: indent.go:ShouldIndentChildNode/NodeWillIndentChild`
- [ ] `pub fn find_first_non_whitespace_column(...)` + `find_first_non_whitespace_character_and_column`　`// Go: indent.go:FindFirstNonWhitespaceColumn`
- [ ] 内部：`getCommentIndent`/`getBlockIndent`/`getSmartIndent`/`getIndentationForNodeWorker`/`getActualIndentationFor*`/`deriveActualIndentationFromList`/`getListBy{Position,Range}`/`getVisualListRange`/`isControlFlowEndingStatement`/`childIsUnindentedBranchOfConditionalExpression`/`argumentStartsOnSameLineAsPreviousArgument` 等全量逐函数　`// Go: indent.go:<Func>`

### `span.rs`（Go: `internal/format/span.go`）

- [ ] `fn find_enclosing_node(range, file) -> NodeId`　`// Go: span.go:findEnclosingNode`
- [ ] `fn get_scan_start_position(...)` / `fn get_own_or_inherited_delta(...)`　`// Go: span.go:getScanStartPosition/getOwnOrInheritedDelta`
- [ ] `fn prepare_range_contains_error_function(diags, range) -> impl Fn(TextRange)->bool`　`// Go: span.go:prepareRangeContainsErrorFunction`
- [ ] `struct FormatSpanWorker` + `new_format_span_worker(...)`　`// Go: span.go:newFormatSpanWorker`
- [ ] `fn execute(&mut self, scanner) -> Vec<TextChange>`（主循环）　`// Go: span.go:execute`
- [ ] `process_node` / `process_child_node` / `process_child_nodes` / `execute_process_node_visitor`　`// Go: span.go:processNode...`
- [ ] `process_pair`（查规则 + applyRuleEdits → `LineAction`）/ `apply_rule_edits` / `process_range` / `process_trivia`　`// Go: span.go:processPair/applyRuleEdits`
- [ ] `compute_indentation` / `try_compute_indentation_for_list_item` / `get_dynamic_indentation`　`// Go: span.go:computeIndentation...`
- [ ] trim 空白族：`trim_trailing_whitespaces_for_remaining_range` / `...for_positions` / `...for_lines` / `get_trailing_whitespace_start_position`　`// Go: span.go:trimTrailing*`
- [ ] 缩进/编辑族：`insert_indentation` / `character_to_column` / `indentation_is_different` / `indent_trivia_items` / `indent_multiline_comment` / `get_indentation_string`　`// Go: span.go:insertIndentation...`
- [ ] 记录编辑：`record_delete` / `record_replace` / `record_insert` / `create_text_change_from_start_length` / `consume_token_and_advance_scanner`　`// Go: span.go:recordDelete...`
- [ ] `struct DynamicIndenter` + 方法 `get_indentation_for_comment`/`get_indentation_for_token`/`get_indentation`/`get_delta`/`recompute_indentation`/`should_add_delta`　`// Go: span.go:dynamicIndenter`
- [ ] `fn get_first_non_decorator_token_of_node` / `get_non_decorator_token_pos_of_node` / `is_comment` / `is_string_or_regular_expression_or_template_literal`　`// Go: span.go:getFirstNonDecoratorTokenOfNode...`
- [ ] `LineAction` enum（None/LineAdded/LineRemoved，对齐 span.go 内定义）

### `api.rs`（Go: `internal/format/api.go`）

- [ ] `#[repr(i32)] pub enum FormatRequestKind { FormatDocument, FormatSelection, FormatOnEnter, FormatOnSemicolon, FormatOnOpeningCurlyBrace, FormatOnClosingCurlyBrace }`　`// Go: api.go:FormatRequestKind`
- [ ] **设置传递（偏离）**：`get_format_code_settings`/`get_new_line_or_default`（参数化，不用 context）　`// Go: api.go:GetFormatCodeSettingsFromContext/GetNewLineOrDefaultFromContext`
- [ ] `pub fn format_span(opts, span, file, kind) -> Vec<TextChange>`　`// Go: api.go:FormatSpan`
- [ ] `pub fn format_node_given_indentation(opts, node, file, lang_variant, initial_indent, delta) -> Vec<TextChange>`　`// Go: api.go:FormatNodeGivenIndentation`
- [ ] `pub fn format_document(opts, file) -> Vec<TextChange>`　`// Go: api.go:FormatDocument`
- [ ] `pub fn format_selection(opts, file, start, end) -> Vec<TextChange>`　`// Go: api.go:FormatSelection`
- [ ] `pub fn format_on_opening_curly` / `format_on_closing_curly` / `format_on_semicolon` / `format_on_enter`　`// Go: api.go:FormatOn*`
- [ ] `fn format_node_lines(opts, file, node, kind) -> Vec<TextChange>`　`// Go: api.go:formatNodeLines`

### Cargo / crate 接线

- [ ] `internal/format/Cargo.toml`（`name = "tsgo_format"` + path deps：ast/astnav/core/scanner/debug/stringutil/ls_lsutil）
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/format`
- [ ] `lib.rs` 声明 `mod api; mod context; mod rule; mod rulecontext; mod rules; mod rulesmap; mod scanner; mod span; mod indent; mod util;` + `pub use` 入口函数与 `FormatRequestKind`

## TDD 推进顺序（tracer bullet → 增量）

1. `util.rs::get_line_start_position_for_position` + `indent.rs::get_indentation`（无规则依赖）→ 过 `indent_getindentation_test.go`（期望 4）。这是最小可验证片段。
2. `indent.rs::get_containing_list` → 过 `indent_test.go::TestGetContainingList_NamedImports`（命名导入列表）。
3. `rule.rs` + `rules.rs::get_all_rules` + `rulesmap.rs` → 规则表能构建（断言 bucket 非空）。
4. `scanner.rs` + `context.rs` → token/trivia 流正确。
5. `span.rs::execute` 全链路 → 过 `format_test.go::TestFormatNoTrailingSpace`（8 子用例，断言无行尾空白）。
6. `api.rs` 全部入口 → 过 `comment_test.go`（注释/JSDoc 稳定性 + 不 panic）、`TestSliceBoundsPanic`、`TestFormat`（checker.ts，需 TS submodule，否则 skip）。

## 与 Go 的已知偏离（divergence）

1. **context.Context → 显式参数**：`WithFormatCodeSettings`/`GetFormatCodeSettingsFromContext`/`GetNewLineOrDefaultFromContext` 不复刻 Go 的 context value 透传，改成把 `&FormatCodeSettings` 与 `new_line: &str` 直接传参。Go 注释自承 strada 把 rulesMap 既全局缓存又塞 context「for some reason」，我们只用全局。
2. **`node.Parent` → `node.parent(arena)`**：全局 AST 所有权约定（PORTING §5）。
3. **全局规则表**：`sync.OnceValue(buildRulesMap)` → `OnceLock`/`Lazy`；规则是 `&'static`，谓词是 `fn` 指针。
4. **panic 语义保留**：`UpdateContext` 三处 nil panic、`getRuleBucketIndex` 的 `debug.Assert`、`increaseInsertionIndex` 的「最多 32 条/桶」断言 → `assert!`/`debug_assert!`/`panic!` 同条件。`TestSliceBoundsPanic`/`comment_test.go` 专门验证「不 panic」，移植时切片边界须严格对齐 Go（用 `get`/显式范围检查）。

## 转交 / 推迟（DEFER）

- 无跨 phase 阻塞：依赖的 `ast`/`scanner`/`astnav`/`lsutil` 均在 P7 之前或同 phase。
- 规则表正确性的**细粒度** parity（每条规则在真实代码上的效果）由 P10 fourslash + `tests/cases/fourslash/*format*` 兜底；本包单测只覆盖入口级行为，见 `tests.md`。
