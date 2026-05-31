# parser: 实现方案（impl.md）

**crate**：`tsgo_parser`　**目标**：把 scanner 产出的 token 流递归下降解析成 `ast.SourceFile`（完整 AST + 诊断 + JSDoc + pragma + 外部模块引用收集），是 TS/JS/JSON/JSX/`.d.ts` 的统一入口。
**依赖（crate）**：`tsgo_ast` `tsgo_scanner` `tsgo_core` `tsgo_collections` `tsgo_debug` `tsgo_diagnostics` `tsgo_tspath`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/parser/`（6 个非测试文件：`parser.go` 271KB / `jsdoc.go` 45KB / `reparser.go` 25KB / `references.go` 3.7KB / `utilities.go` 2.2KB / `types.go` 0.4KB）

## 这个包是什么（业务说明）

parser 是编译器的**语法分析层**：输入 `(SourceFileParseOptions, sourceText, ScriptKind)`，输出一个 `*ast.SourceFile`。它持有一个 `*scanner.Scanner`（向其注入 `scanError` 回调）和一个 `ast.NodeFactory`（节点工厂/arena），用经典**递归下降 + 运算符优先级**算法逐个 token 构造节点，并在过程中：

- **构建并下挂诊断**（语法错误恢复：缺失 token 用"missing node"占位，不中断解析）；
- **解析 JSDoc**（`jsdoc.go`）并把 `@type`/`@param`/`@typedef` 等**重解析（reparse）**为等价 TS 语义节点（`reparser.go`，仅 JS 文件）；
- **收集 pragma / triple-slash 指令 / `@ts-ignore` 指令**（来自 scanner 的 commentDirectives）；
- **收集外部模块引用**（`references.go`：`import`/`require`/动态 `import()`/ambient module）供后续 module resolution；
- 识别**外部模块指示器**与**顶层 await**（必要时整文件 reparse 一次）。

核心机制有四个：(1) **`NodeFactory`**：所有节点经 `p.factory.NewXxx(...)` 在 arena 上分配；(2) **`finishNode`**：设节点 `Loc`（`[pos,end)`，pos 来自 `scanner.TokenFullStart()`）、并入 `contextFlags`、回填**直接子节点的 Parent**；(3) **lookahead/speculation**：`p.mark()/p.rewind()`（含 scanner state）做投机解析（如箭头函数 vs 括号表达式）；(4) **parsingContexts**：`parseList`/`parseDelimitedList` 按上下文（25 种 `ParsingContext`）决定列表元素/终止符/分隔符与错误恢复。

为什么在 P3：parser 直接依赖 scanner（同 P3）与 ast（P2），是 binder（同 P3）与 checker（P4）的前置。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。AST 用 **arena + `NodeId`** 已在 P2 `ast` 落地，parser 复用之。本包特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type Parser struct { scanner *Scanner; factory ast.NodeFactory; ... }` | `struct Parser<'a> { scanner: Scanner, factory: NodeFactory, ... }` | parser 是**短命可变状态机**，不是图节点；直接用普通 struct + `&mut self` 方法。`factory` 持有节点 arena（P2 定义）。 |
| `sync.Pool`（parserPool/复用 Parser） | `thread_local!` 池 或干脆每次 `Parser::new()` | Go 用 pool 摊销分配。Rust 第一版可不池化（`// PERF(port): parser pool`）；若热路径需要，用 `thread_local!{ static POOL: RefCell<Vec<Parser>> }`。`putParser` 的"清空但保留 scanner/闭包"对应重置字段。 |
| `p.factory.NewXxx(...)` → `*ast.Node` | `self.factory.new_xxx(...) -> NodeId` | **NodeFactory 用法**：每个产生式调用工厂方法拿到节点（P2 里是 `NodeId` 索引）。本包不直接 new struct，全部走 factory，保证 arena 归属与 `Kind` 一致。 |
| `finishNode(node, pos)`：设 `node.Loc`、并 `contextFlags`、`overrideParentInImmediateChildren` | `fn finish_node(&mut self, id: NodeId, pos: i32) -> NodeId` | `node.Loc = [pos, nodePos()]`；`flags |= contextFlags`；若 `hasParseError` 置 `ThisNodeHasError`；遍历直接子节点写 `parent`。**Parent 用 `NodeId`**：`arena[child].parent = id`（不是 `&`）。 |
| `node.Parent = p.currentParent`（闭包 `setParentFromContext`） | `arena[child].parent = self.current_parent`（在 `for_each_child` visitor 里） | Go 用一个复用闭包 + `ForEachChild`。Rust：`finish_node` 里 `self.current_parent = id; node.for_each_child(|c| arena[c].parent = id)`。零 `unsafe`。 |
| `token ast.Kind`（当前 token 缓存） | `token: Kind` | parser 镜像 scanner 当前 token；`next_token()` = `self.token = self.scanner.scan()`。 |
| `contextFlags ast.NodeFlags`（Yield/Await/Ambient/JS…） | `context_flags: NodeFlags`（`bitflags`） | 进入/退出某语境时 set/clear，`finish_node` 并入每个节点。投机解析时随 `ParserState` 保存/恢复。 |
| `parsingContexts ParsingContexts`（位集） | `parsing_contexts: ParsingContexts`（`bitflags` over `1<<ParsingContext`） | `parseList` 时置位、错误恢复 `isInSomeParsingContext` 时查位。 |
| `ParsingContext`（iota 枚举 25 项） | `#[repr(i32)] enum ParsingContext { SourceElements=0, ... Count }` | 直译，顺序/取值必须一致（影响 `1<<ctx` 位集）。 |
| `ParseFlags`（types.go，位枚举） | `bitflags! ParseFlags`（Yield/Await/Type/IgnoreMissingOpenBrace/JSDoc） | 见 types.go。 |
| `identifiers map[string]string`（标识符 interning） | `FxHashMap<String,String>`（或 `&str` interner） | 同字符串去重以省内存；语义=返回同一份 String。 |
| `notParenthesizedArrow collections.Set[int]` | `FxHashSet<i32>` | 记"某 pos 已判定不是括号箭头函数"避免重复投机。键是 token 起始 pos。 |
| `nodeSliceArena core.Arena[*ast.Node]` / `stringSliceArena` | `Arena<NodeId>` / `Arena<String>`（P1 `core::Arena`） | 列表节点的子 slice 在 arena 上批量分配，避免大量小 `Vec`。`// PERF(port)`：第一版可用 `Vec`。 |
| `lookAhead/tryParse`（投机，传函数） | `fn look_ahead<R>(&mut self, f: impl FnOnce(&mut Parser)->R) -> R` | 内部 `mark()` → 运行 `f` → `rewind()`（lookahead 总回滚；tryParse 成功不回滚）。**借用注意**：`f` 接 `&mut Parser`，Go 传 `func(p *Parser)`，等价。 |
| `ParserState`（mark/rewind 快照） | `#[derive(Clone,Copy)] struct ParserState`（含 `scannerState` + 各 len 计数 + flags） | 投机失败时把 `diagnostics`/`jsDiagnostics`/`jsdocInfos`/`reparsedClones` **按记录长度截断**回滚（Go 同此）。 |
| `expressions any`（`[]*Expression \| *Expression`，JSON 解析里） | `enum OneOrMany { One(NodeId), Many(Vec<NodeId>) }` | Go 用 `any` 做"单个或多个"；Rust 用判别枚举。 |
| `*ast.SourceFile` 返回 | `SourceFileId` / `&SourceFile`（P2 定义） | 入口返回；`finishSourceFile` 写诊断/pragma/CommonJS 指示器等。 |

### NodeFactory / finishNode 用法（要点说明）

每条产生式的骨架（以二元表达式为例）：
```text
pos := p.nodePos()                 // = scanner.TokenFullStart()，含前导 trivia 起点
left := p.parseUnaryExpression()
op := p.parseTokenNode()           // 工厂建运算符 token 节点
right := p.parseBinaryExpressionOrHigher(prec)
node := p.factory.NewBinaryExpression(left, op, right)   // arena 分配
return p.finishNode(node, pos)     // 设 [pos,end)、并 contextFlags、回填子节点 parent
```
- `nodePos()` 取 `scanner.TokenFullStart()`（**含前导 trivia**），`finishNode` 的 end 取下一次 `nodePos()`（即当前 token 全启位置），所以节点 `Pos()` 含 leading trivia、`End()` 不含 trailing trivia——与 TS `node.pos/node.end` 语义一致，astnav/scanner 的 `GetTokenPosOfNode` 据此 `SkipTrivia`。
- `newNodeList(loc, nodes)` 给列表节点单独的 `Loc`（含分隔符范围）。
- `missingListNodes` 哨兵区分"缺失列表"（期望的 `(`/`{` 没出现）与"空列表"——Rust 用 `Option<NodeList>` 或专门 flag（不能靠指针相等，见偏离）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/parser/parser.go` | `internal/parser/lib.rs`（crate 根） | `Parser` 结构、`ParseSourceFile`/`ParseIsolatedEntityName` 入口、`parseJSONText`、全部 statement/declaration/expression/type/binding/jsx 产生式、`parseList`/`parseDelimitedList`、`nextToken`/`finishNode`/`mark`/`rewind`/`lookAhead`、pragma/外部模块指示器/顶层 await reparse。`mod jsdoc; mod reparser; mod references; mod types; mod utilities;` + 公开 re-export。 |
| `internal/parser/jsdoc.go` | `internal/parser/jsdoc.rs` | JSDoc 注释解析：`withJSDoc`、`parseJSDocComment(Worker)`、`parseTag`/各 `@tag`、`parseJSDocTypeExpression`/`parseJSDocSignature`/`parseJSDocLink*`/`parseJSDocEntityName`。全是 `Parser` 方法。 |
| `internal/parser/reparser.go` | `internal/parser/reparser.rs` | 把 JS 文件里的 JSDoc 标签重写为等价 TS 节点：`reparseTags`/`reparseHosted`/`reparseUnhosted`/`reparseJSDocSignature`/`reparseJSDocTypeLiteral`、`addDeepCloneReparse`、`gatherTypeParameters`、`makeNewCast`、`makeQuestionIfOptional`。`reparsedClones` 去重链。 |
| `internal/parser/references.go` | `internal/parser/references.rs` | `collectExternalModuleReferences`/`collectModuleReferences`：扫顶层 import/export/ambient module/动态 import，填 `file.Imports/ModuleAugmentations/AmbientModuleNames/UsesUriStyleNodeCoreModules`。 |
| `internal/parser/types.go` | `internal/parser/types.rs` | `ParseFlags`（bitflags）。 |
| `internal/parser/utilities.go` | `internal/parser/utilities.rs` | `getLanguageVariant`、`tokenIsIdentifierOrKeyword(OrGreaterThan)`、`GetJSDocCommentRanges`、`isKeywordOrPunctuation`、`isJSDocLikeText`。 |

> **无源码子包**：`ls internal/parser` 仅见 `testdata/`（其下 `fuzz/FuzzParser/<corpus>`）。这些是 **fuzz 语料目录**，不是 Go 子包（无 `.go`）。详见"已知偏离"。

## 依赖白名单（本包新增的 crate）

- `rustc_hash`（`FxHashMap`/`FxHashSet`）——标识符 interning、`notParenthesizedArrow`。
- `bitflags`——`ParseFlags`/复用 `NodeFlags`（P2）。
- 不引入解析器生成器：**手写递归下降**，1:1 对齐 Go 控制流。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按 TDD 推进序：入口骨架 → 表达式 → 语句/声明 → 类型 → JSX → JSDoc/reparse → references/pragma。

### `lib.rs` — 状态与入口（Go: `parser.go`）

- [x] `enum ParsingContext`（26 项，全枚举，bit 值对齐 Go；未触达上下文 `#[allow(dead_code)]`）　`// Go: parser.go:ParsingContext`
- [x] `struct Parser`（本轮所需字段子集：scanner/arena/opts/token/contextFlags/parsingContexts/diagnostics/scan_errors…）+ `new()`　`// Go: parser.go:Parser/newParser`
- [x] `pub fn parse_source_file(opts, source_text, script_kind) -> ParseResult`（`initialize_state`→`next_token`→worker；返回 `ParseResult{arena,source_file,diagnostics}`，因 arena 须随返回值移交）　`// Go: parser.go:ParseSourceFile`
- [x] `pub fn parse_isolated_entity_name(text) -> Option<(NodeArena, NodeId)>`　`// Go: parser.go:ParseIsolatedEntityName`
- [x] `fn initialize_state(opts, text, kind)`（建 scanner、set contextFlags by kind、接 `scan_errors` sink；JSON 走 JSX 语言变体）　`// Go: parser.go:initializeState`
- [ ] `fn parse_json_text(&mut self) -> SourceFile`（含 `OneOrMany` 错误恢复、`validateJsonValue/validateJsonObjectLiteral`）　`// Go: parser.go:parseJSONText/validateJson*`　— DEFER(phase-3): object literal + JSON value 校验
- [x] `fn parse_source_file_worker(&mut self) -> ParseResult`（`parse_list_index(SourceElements)`→EOF→`new_source_file`→`finish_node`；reparseList/顶层 await reparse/`collectExternalModuleReferences` 暂未接入）　`// Go: parser.go:parseSourceFileWorker`
- [ ] `fn finish_source_file(...)`（commentDirectives/pragmas/diagnostics/CommonJS 指示器）　`// Go: parser.go:finishSourceFile`　— DEFER(phase-3)

### `lib.rs` — token 流与诊断（Go: `parser.go`）

- [x] `fn next_token(&mut self)->Kind`（关键字含转义 → 报错）/ `next_token_without_check`（含 `drain_scan_errors`）；`next_token_jsdoc`/`next_jsdoc_comment_text_token` 暂未接入　`// Go: parser.go:nextToken/...`
- [x] `fn node_pos(&self)->i32`（=`scanner.token_full_start()`）/ `has_preceding_line_break`　`// Go: parser.go:nodePos/hasPrecedingLineBreak`
- [x] 诊断族：`parse_error_at/parse_error_at_current_token/parse_error_at_range`（去重：同 pos 不重复报）+ scanner `scan_errors` sink 经 `drain_scan_errors` 注入　`// Go: parser.go:scanError/parseErrorAt*`
- [ ] `struct ParserState` + `fn mark()->ParserState` / `fn rewind(s)`（含 scanner state + 各 len 截断）　`// Go: parser.go:ParserState/mark`　— DEFER(phase-3): 随首个需要投机的产生式（arrow/JSON）落地
- [ ] `fn look_ahead<R>(f)` / `fn try_parse<R>(f)`（投机；lookahead 必回滚，tryParse 成功保留）　`// Go: parser.go:lookAhead/tryParse`　— DEFER(phase-3)
- [x] `fn parse_optional(token)->bool` / `parse_expected(kind)->bool` / `parse_expected_token(kind)->NodeId` / `parse_token_node()->NodeId` / `parse_optional_token`　`// Go: parser.go:parseOptional/parseExpected/...`

### `lib.rs` — 列表与节点收尾（Go: `parser.go`）

- [x] `fn parse_list_index` / `parse_delimited_list(ctx, parse_element)->NodeList`（分隔符、错误恢复子集：SourceElements/BlockStatements/ArgumentExpressions/ArrayLiteralMembers）；`parse_list` 包装待 block/switch 落地　`// Go: parser.go:parseList/parseDelimitedList`
- [ ] `fn create_missing_list()->NodeList`（`missingListNodes` 哨兵 → Rust 用 `is_missing` 标志）　`// Go: parser.go:createMissingList/isMissingNodeList`　— DEFER(phase-3)
- [x] `fn new_node_list(loc, nodes)`；`new_modifier_list` 待 modifiers 落地　`// Go: parser.go:newNodeList/newModifierList`
- [x] `fn finish_node(id, pos)->NodeId` / `finish_node_with_end(id, pos, end)->NodeId`（设 Loc/flags/parent 回填）　`// Go: parser.go:finishNode/finishNodeWithEnd`
- [x] `fn override_parent_in_immediate_children(id)`（`for_each_child` 写 `arena[c].parent=id`）　`// Go: parser.go:overrideParentInImmediateChildren`

### `lib.rs` — 产生式（按 TS 语法逐族；逐方法带 `// Go: parser.go:parseXxx`）

> 这是工作量主体。按族列粗粒度 TODO，执行期逐 `parseXxx` 1:1（Go 里约 300+ 个 `parse*` 方法）。

- [ ] **表达式**：`parseExpression`/`parseAssignmentExpressionOrHigher`/`parseConditional`/`parseBinaryExpressionOrHigher`(优先级爬升)/`parseUnary`/`parsePostfix`/`parseLeftHandSide`/`parseMemberExpressionOrHigher`/`parseCallExpression`/`parsePrimaryExpression`/`parseArrowFunction`(投机) 等　`// Go: parser.go:parse*Expression*`
- [ ] **字面量**：`parseLiteralExpression`/`parseTemplateExpression`/`parseArrayLiteralExpression`/`parseObjectLiteralExpression`/`parsePrefixUnaryExpression`　`// Go: parser.go:parse*Literal*`
- [ ] **语句**：`parseStatement`/`parseToplevelStatement`/`parseBlock`/`parseIf/Do/While/For/Switch/Try/Return/Throw/Break/Continue/Labeled/With/Debugger/Empty/ExpressionStatement`/`parseVariableStatement`　`// Go: parser.go:parse*Statement*`
- [ ] **声明**：`parseDeclaration`/`parseFunctionDeclaration`/`parseClassDeclaration(Expression)`/`parseInterfaceDeclaration`/`parseTypeAliasDeclaration`/`parseEnumDeclaration`/`parseModuleDeclaration`/`parseImport*`/`parseExport*`/`parseClassMember`　`// Go: parser.go:parse*Declaration*`
- [ ] **类型**：`parseType`/`parseTypeWorker`/`parseUnionOrIntersectionType`/`parseTypeOperator`/`parsePostfixType`/`parseNonArrayType`/`parseTypeReference`/`parseFunctionOrConstructorType`/`parseMappedType`/`parseTupleType`/`parseTypeParameters`/`parseTypeArguments`/`parseTypeMember`　`// Go: parser.go:parse*Type*`
- [ ] **绑定/参数**：`parseParameter`/`parseParameterList`/`parseBindingElement`/`parseObjectBindingPattern`/`parseArrayBindingPattern`/`parseModifiers`　`// Go: parser.go:parse*Binding*/parseParameter*/parseModifiers`
- [ ] **JSX**：`parseJsxElementOrSelfClosingElementOrFragment`/`parseJsxOpeningOrSelfClosingElement`/`parseJsxAttributes`/`parseJsxChildren`/`parseJsxExpression`/`parseJsxText`（用 scanner 的 `ReScanJsxToken`/`ScanJsxToken`）　`// Go: parser.go:parseJsx*`
- [ ] **标识符/名字**：`parseIdentifier`/`parseIdentifierName`/`parseEntityName`/`parsePropertyName`/`parseComputedPropertyName`/`parseTokenNode`　`// Go: parser.go:parseIdentifier*/parseEntityName/parsePropertyName`
- [ ] **错误恢复辅助**：`isListElement`/`isListTerminator`/`parsingContextErrors`/`abortParsingListOrMoveToNextToken`/`scanTypeMemberStart`　`// Go: parser.go:isListElement/isListTerminator/...`
- [ ] **pragma/指示器**：`getCommentPragmas`/`processPragmasIntoFields`/`setExternalModuleIndicator`/`reparseTopLevelAwait`/`attachFileToDiagnostics`　`// Go: parser.go:getCommentPragmas/processPragmasIntoFields/...`

### `jsdoc.rs`（Go: `internal/parser/jsdoc.go`）

- [ ] `fn with_jsdoc(node, info)->Vec<NodeId>`（把前导 JSDoc 挂到节点）　`// Go: jsdoc.go:withJSDoc`
- [ ] `fn parse_jsdoc_type_expression(may_omit_braces)->NodeId` / `parse_jsdoc_name_reference`　`// Go: jsdoc.go:parseJSDocTypeExpression/parseJSDocNameReference`
- [ ] `fn parse_jsdoc_comment(parent,start,end,full_start)->NodeId` / `parse_jsdoc_comment_worker(start,end,full_start,indent)->NodeId`　`// Go: jsdoc.go:parseJSDocComment/parseJSDocCommentWorker`
- [ ] `fn parse_tag(tags, margin)->NodeId`（分发各 `@tag`）/ `parse_tag_comments(indent, initial_margin)->NodeList`　`// Go: jsdoc.go:parseTag/parseTagComments`
- [ ] `fn parse_jsdoc_link(start)->NodeId` / `parse_jsdoc_link_name` / `parse_jsdoc_link_prefix`　`// Go: jsdoc.go:parseJSDocLink*`
- [ ] `fn parse_jsdoc_signature(start,indent)->NodeId` / `parse_jsdoc_entity_name(msg)->EntityName` / `parse_jsdoc_identifier_name(msg)->IdentifierNode`　`// Go: jsdoc.go:parseJSDocSignature/parseJSDocEntityName/parseJSDocIdentifierName`

### `reparser.rs`（Go: `internal/parser/reparser.go`）

- [ ] `fn finish_reparsed_node/finish_mutated_node/add_deep_clone_reparse(node)->NodeId`　`// Go: reparser.go:finishReparsedNode/finishMutatedNode/addDeepCloneReparse`
- [ ] `fn reparse_tags(parent, jsdoc)` / `reparse_unhosted(tag,parent,jsdoc)` / `reparse_hosted(tag,parent,jsdoc)`　`// Go: reparser.go:reparseTags/reparseUnhosted/reparseHosted`
- [ ] `fn reparse_jsdoc_signature(...)->NodeId` / `reparse_jsdoc_type_literal(t)->NodeId` / `reparse_jsdoc_comment(node,tag)`　`// Go: reparser.go:reparseJSDoc*`
- [ ] `fn gather_type_parameters(j, tag)->NodeList` / `make_question_if_optional(param)->NodeId` / `make_new_cast(t,e,is_assertion)->NodeId`　`// Go: reparser.go:gatherTypeParameters/makeQuestionIfOptional/makeNewCast`
- [ ] 辅助：`find_matching_parameter` / `skip_satisfies_expressions` / `get_function_like_host` / `get_class_like_data`　`// Go: reparser.go:findMatchingParameter/...`

### `references.rs`（Go: `internal/parser/references.go`）

- [ ] `fn collect_external_module_references(file)`　`// Go: references.go:collectExternalModuleReferences`
- [ ] `fn collect_module_references(file, node, in_ambient_module)`（import/export/ambient module/`node:` 前缀逻辑）　`// Go: references.go:collectModuleReferences`

### `utilities.rs`（Go: `internal/parser/utilities.go`）

- [ ] `fn get_language_variant(kind)->LanguageVariant`　`// Go: utilities.go:getLanguageVariant`
- [ ] `fn token_is_identifier_or_keyword(token)->bool` / `..._or_greater_than`　`// Go: utilities.go:tokenIsIdentifierOrKeyword*`
- [ ] `pub fn get_jsdoc_comment_ranges(factory, ranges, node, text)->Vec<CommentRange>`　`// Go: utilities.go:GetJSDocCommentRanges`
- [ ] `fn is_keyword_or_punctuation(token)->bool` / `is_jsdoc_like_text(text)->bool`　`// Go: utilities.go:isKeywordOrPunctuation/isJSDocLikeText`

### `types.rs`（Go: `internal/parser/types.go`）

- [ ] `bitflags! ParseFlags { Yield, Await, Type, IgnoreMissingOpenBrace, JSDoc }`　`// Go: types.go:ParseFlags`

### Cargo / crate 接线

- [ ] `internal/parser/Cargo.toml`（`name = "tsgo_parser"` + path deps：ast/scanner/core/collections/debug/diagnostics/tspath）
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/parser`
- [ ] `lib.rs` 声明子模块 + 公开 re-export（`parse_source_file`/`parse_isolated_entity_name`/`get_jsdoc_comment_ranges`）

## TDD 推进顺序（tracer bullet → 增量）

1. **入口最小闭环**：`parse_source_file` 解析空源 / 单 `;` / 单标识符表达式语句 → 断言 `SourceFile.Statements` 结构 + `Pos/End`。这打通 scanner 接线 + `finish_node` + parent 回填。
2. **`ParseIsolatedEntityName`**：`a.b.c` → `QualifiedName` 链（最小、无语句机制），验证 `finishNode`/`nodePos` 正确。对齐 Rust 单测。
3. **表达式优先级**：`1 + 2 * 3` → 结构正确；`a ? b : c`；`(x) => x` 投机（mark/rewind）。
4. **语句/声明**：`const x = 1`、`function f(){}`、`class C{}`、`if/for/while`。
5. **类型语法**：`type T = A | B`、`interface I {}`、泛型 `Array<T>`（含 `re_scan_greater_than_token` 接线）。
6. **JSON**：`parse_json_text` + `validate_json_value`（JSON 模式诊断）。
7. **JSDoc + reparse**：`.js` 文件里 `/** @type {string} */` → reparse 成 cast；对齐 `TestJSDocImportTypeParentChain`（见 tests.md）。
8. **references/pragma/外部模块指示器**：import 收集、`/// <reference />`、顶层 await reparse。

## 实现进度（Round 2 — depth-first grammar）

在 Round 1 的表达式骨架之上，Round 2 逐族（red→green，每族一编译）补齐了**几乎整个语句 + 声明语法**，并为每个新节点种类**附加**了 `tsgo_ast` 的 `NodeData` variant + 构造器 + `for_each_child` + `visit_each_child`（additive，未破坏既有公开 API；`cargo test/clippy -p tsgo_ast` 全绿）。

**已落地（全绿：`cargo test -p tsgo_parser -p tsgo_ast` + `clippy -D warnings` + gate C1–C8）**：

- **语句全族**：`if`/`else`、`while`、`do`、`for`/`for-in`/`for-of`(+`for await`)、`switch`(+`case`/`default`/`CaseBlock`)、`try`/`catch`/`finally`、`throw`、`return`、`break`/`continue`(带 label)、`block`、`labeled`、`with`、`debugger`、`empty`、`expression`。
- **变量 + 绑定**：`var`/`let`/`const`（`VariableStatement`/`VariableDeclarationList`(let/const flag)/`VariableDeclaration`，含 `!` 明确赋值、类型注解、初始化）、对象/数组解构（`ObjectBindingPattern`/`ArrayBindingPattern`/`BindingElement`/`OmittedExpression`/`ComputedPropertyName`/私有标识符）。
- **函数 + 参数 + 修饰符**：`FunctionDeclaration`（含 `export`/`async`/`declare` 修饰符、泛型 `<T>`、参数、返回类型、body/overload-semicolon）、`ParameterDeclaration`（`...`rest/`?`optional/type/默认值）、`TypeParameterDeclaration`（`extends`/`=`）、完整 `parseModifiers`/`tryParseModifier`/`nextTokenCanFollowModifier` 族、`parseDeclaration`/`parseDeclarationWorker`/`scanStartOfDeclaration`。
- **类**：`ClassDeclaration`/`ClassExpression`、heritage（`extends`/`implements` → `HeritageClause`/`ExpressionWithTypeArguments`）、成员全族：`PropertyDeclaration`/`MethodDeclaration`/`GetAccessor`/`SetAccessor`/`Constructor`/`IndexSignature`/`ClassStaticBlockDeclaration`/`SemicolonClassElement`（含 `scanClassMemberStart` + `static {}` 块停止逻辑）。
- **interface / type alias / enum**：`InterfaceDeclaration`、`TypeAliasDeclaration`、`EnumDeclaration`/`EnumMember`、类型成员 `PropertySignature`/`MethodSignature`/`CallSignature`/`ConstructSignature`/`IndexSignature` + `TypeLiteral`（`scanTypeMemberStart`）。
- **module / namespace**：`ModuleDeclaration`(module/namespace/global) + `ModuleBlock` + 嵌套 `a.b` 的隐式 export 修饰符 + ambient external module(`declare module "x"`)。
- **import / export 全形态**：`import "m"` / `import x from` / `import * as ns` / `import {a, b as c}` / `import type ...` / `import x = require(...)` / `import A = B.C`；`export {..}` / `export {..} from` / `export * from` / `export * as ns from` / `export =` / `export default` / `export as namespace X`（节点：`ImportDeclaration`/`ImportClause`/`NamespaceImport`/`NamedImports`/`ImportSpecifier`/`ExternalModuleReference`/`ImportEqualsDeclaration`/`ExportDeclaration`/`NamedExports`/`NamespaceExport`/`ExportSpecifier`/`ExportAssignment`/`NamespaceExportDeclaration`）。
- **reduced 类型解析**（足够覆盖 var/param/return/alias 注解）：keyword types(以 child-less `Token` 表示)、`TypeReference`(+type args)、`ArrayType`、`IndexedAccessType`、union/intersection、`ParenthesizedType`、`LiteralType`、`TypeLiteral`，+ `parseTypeAnnotation`/`parseTypeParameters`/`parseBracketedList`/`reScanLessThanToken`/`isStartOfType`。
- 列表上下文扩展：`BlockStatements`/`SwitchClauses`/`SwitchClauseStatements`/`VariableDeclarations`/`ObjectBindingElements`/`ArrayBindingElements`/`Parameters`/`TypeParameters`/`TypeArguments`/`ClassMembers`/`HeritageClauses`/`HeritageClauseElement`/`TypeMembers`/`EnumMembers`/`ImportOrExportSpecifiers`（`isListElement`/`isListTerminator`/`isInSomeParsingContext`）。
- 投机基础设施 `mark`/`rewind`/`look_ahead`(+`ParserState`) 落地（被 `is_let_declaration`/modifier-lookahead/index-signature/constructor 等使用）。
- deepclone 回填扩展到 ~60 个真实解析 case（覆盖上述新节点种类）。

**仍 DEFER(phase-3)**（已 `// DEFER` + `// blocked-by:` 标注）：arrow/yield 投机、JSX、JSDoc/reparse、JSON、`as`/`satisfies` 表达式（需 `AsExpression`/`SatisfiesExpression` 节点）、function/constructor/conditional/operator/infer/tuple/mapped/typeof/import/template/predicate 类型节点、import attributes(`with{}`)、`defer` import 阶段修饰符、specifier 级 `type as as` 消歧、装饰器、`MissingDeclaration` 错误恢复、`finishSourceFile`/pragma/外部模块指示器/references。

## 实现进度（Round 3 — 表达式 + 完整类型语法）

在 Round 2 的语句/声明语法之上，Round 3 逐族（red→green，每族一编译）补齐了**剩余表达式**与**完整类型语法**，并为每个新节点种类**附加**了 `tsgo_ast` 的 `NodeData` variant + 构造器 + `for_each_child` + `visit_each_child`（additive，未破坏既有公开 API；`cargo test/clippy -p tsgo_ast` 全绿）。

**已落地（全绿：`cargo test -p tsgo_parser -p tsgo_ast` + `clippy -D warnings`；本 crate gate C1–C8 全绿）**：

- **对象字面量**：`ObjectLiteralExpression` + `PropertyAssignment`/`ShorthandPropertyAssignment`(含 cover-initialized `=`)/`SpreadAssignment` + 方法/`get`/`set` 访问器 + 计算键。
- **一元关键字 + yield**：`delete`/`typeof`/`void`(`DeleteExpression`/`TypeOfExpression`/`VoidExpression`)、`await`(`AwaitExpression`，含 `isAwaitExpression` 启发式)、`yield`/`yield*`(`YieldExpression`，含 `isYieldExpression`)；函数 body 现按 signature flags 设置 yield/await 上下文（`parse_function_block` + `signature_flags`）。
- **函数表达式**：`FunctionExpression`（含 `*`/`async`/可选名/泛型/返回类型）。
- **箭头函数 + 投机**：`ArrowFunction` + 完整 `mark`/`rewind`/`look_ahead` 三态消歧（`isParenthesizedArrowFunctionExpression`/`nextIsParenthesizedArrowFunctionExpression`/`tryParseParenthesizedArrowFunctionExpression`/`parseParenthesizedArrowFunctionExpression`/`parseSimpleArrowFunctionExpression`/`tryParseAsyncSimpleArrowFunctionExpression`/`parseArrowFunctionExpressionBody`/`parseModifiersForArrowFunction`），覆盖 `x=>`、`(a,b)=>`、`(a):T=>`、`async (a)=>`、`async x=>`、块/简洁 body。
- **LHS 扩展**：可选链 `?.`（属性/元素/调用 + `OPTIONAL_CHAIN` flag）、非空断言 `!`(`NonNullExpression`)、模板 `TemplateExpression`/`TemplateSpan`/`TemplateHead`/`TemplateMiddle`/`TemplateTail`/`NoSubstitutionTemplateLiteral` + 标签模板 `TaggedTemplateExpression`、正则 `RegularExpressionLiteral`(`reScanSlashToken`)、`new`/`NewExpression` + `new.target`/`import.meta`(`MetaProperty`)、私有标识符 primary。
- **`as`/`satisfies` + 类型断言**：`AsExpression`/`SatisfiesExpression`（`parse_binary_expression_rest` 内）、`<T>expr` `TypeAssertionExpression`（非 JSX）。
- **完整类型语法**：函数/构造器类型(`FunctionType`/`ConstructorType` + `isStartOfFunctionTypeOrConstructorType`/`skipParameterStart`/`nextIsUnambiguouslyStartOfFunctionType`)、条件类型 + `infer`(`ConditionalType`/`InferType` + `DisallowConditionalTypesContext` + `tryParseConstraintOfInferType`)、`keyof`/`unique`/`readonly` 运算符(`TypeOperator`)、tuple(`TupleType`/`NamedTupleMember`/`RestType` + `scanStartOfNamedTupleElement`)、mapped(`MappedType` + `nextIsStartOfMappedType`/`parseMappedTypeParameter`)、模板字面量类型(`TemplateLiteralType`/`TemplateLiteralTypeSpan`)、import 类型(`ImportType`)、`typeof` 查询(`TypeQuery`)、类型谓词(`TypePredicate`：`x is T`/`asserts x is T`/`this is T`，经 `parseTypeOrTypePredicate`/`parseAssertsTypePredicate`/`parseThisTypePredicate`)、`this` 类型(`ThisType`)。`parse_return_type` 现走 `parseTypeOrTypePredicate`。
- 列表上下文扩展：`ObjectLiteralMembers`、`TupleElementTypes`（`isListElement`/`isListTerminator`/`isInSomeParsingContext`）。
- deepclone 回填扩展到 ~95 个真实解析 case（覆盖上述全部新节点种类）。

**仍 DEFER(phase-4)**（已 `// DEFER(phase-3)` + `// blocked-by:` 标注）：JSX、JSDoc + reparser（含 `TestJSDocImportTypeParentChain`）、JSON(`parseJSONText`)、references/pragmas/外部模块指示器/`finishSourceFile`/顶层 await reparse、装饰器、import attributes(`with {}`)；以及若干精修：`tryParseTypeArgumentsInExpression`（表达式中泛型实参 `f<T>()`）、`tryReparseOptionalChain`（`!` 后传播可选链）、JSDoc 风格后缀类型(`?T`/`!T`/`*`)、负字面量类型(`-1`)、import-type 的 `with {}` 属性、`parseParametersWorker` 的 `allowAmbiguity` nil 传播（含括号-对象-字面量含方法的边界情形）、`typeHasArrowFunctionBlockingParseError` 精修。

## 实现进度（Round 4 — 收尾：JSX / JSON / 装饰器 / import 属性 / 模块引用 / 精修）

Round 4 在 Round 3 的完整表达式+类型语法之上，补齐了 grammar 的剩余面，使 parser 达到**可供 binder/checker 构建的实质完整度**。每个新节点种类都**附加**了 `tsgo_ast` 的 `NodeData` variant + 构造器 + `for_each_child` + `visit_each_child`（additive，`tsgo_ast` 全程全绿）。

**已落地（全绿：`cargo test -p tsgo_parser -p tsgo_ast` + `clippy -D warnings` + gate C1–C8 workspace 全绿）**：

- **精修（Slice 1）**：`tryParseTypeArgumentsInExpression`（`f<T>()` 表达式中泛型实参 + `canFollowTypeArgumentsInExpression`，含 instantiation expression `f<T>`）、`tryReparseOptionalChain`（`a?.b!.c` 经非空表达式传播可选链）、负字面量类型(`type N = -1;`)、以及**括号-对象-字面量含方法的箭头消歧**（`({ m() {} })`）——通过 Go-faithful 的 `parseParameterEx(allowAmbiguity)` nil 传播 + `parseDelimitedList` 的 `None` 短路（新增 `parse_delimited_list_opt`）。
- **装饰器（Slice 2）**：`Decorator` 节点 + `parseDecorator`/`parseDecoratorExpression`，完整 `parseModifiersEx(allowDecorators, permitConstAsModifier, stopOnStartOfClassStaticBlock)` 三参签名 + leading/trailing decorator 状态机；类声明/类成员/参数/对象字面量方法的装饰器，以及 `parseDecoratedExpression`（`@dec class {}` 表达式）。
- **import 属性（Slice 3）**：`ImportAttributes`/`ImportAttribute` 节点 + `tryParseImportAttributes`/`parseImportAttributes`/`parseImportAttribute`，接入 import 声明、export 声明、import 类型（`import("m", { with: {...} })`），含 `assert` → `with` 的迁移诊断。
- **JSX（Slice 4）**：完整 JSX —— `JsxElement`/`JsxSelfClosingElement`/`JsxOpeningElement`/`JsxClosingElement`/`JsxFragment`/`JsxOpeningFragment`/`JsxClosingFragment`/`JsxAttributes`/`JsxAttribute`/`JsxSpreadAttribute`/`JsxNamespacedName`/`JsxExpression`/`JsxText` 共 13 个节点 + 全套 `parseJsx*` 函数（element/fragment/self-closing、属性 + spread、表达式容器、JSX 文本、嵌套子元素、成员/命名空间标签名、类型实参）。接入 `parseUpdateExpression`（`<` in JSX）与 `parseSimpleUnaryExpression`（mustBeUnary）。scanner JSX 方法（`scan_jsx_token`/`scan_jsx_identifier`/`scan_jsx_attribute_value`/`re_scan_jsx_token`）+ `TagNamesAreEquivalent`。
- **JSON（Slice 5）**：`parseJSONText` + `validateJsonValue`/`validateJsonObjectLiteral`/`isDoubleQuotedString`/`getErrorSpanForNode`，含顶层 array/字面量/对象、`OneOrMany` 多表达式错误恢复、双引号键/合法 JSON 值校验诊断。`parse_source_file_worker` 按 `ScriptKind::Json` 分派。
- **模块引用 + finishSourceFile（Slice 6）**：`SourceFileData.imports` 字段 + `collectExternalModuleReferences`/`collectModuleReferences`（收集 import/re-export 的模块说明符）+ `setExternalModuleIndicator`（`isAnExternalModuleIndicatorNode`：export 修饰符 / import / export / `import x = require()`），接入 `finishSourceFile`（worker + JSON 路径都调用）。这解锁了 binder 的模块判定（`external_module_indicator` + `imports`）。
- 列表上下文扩展：`JsxAttributes`、`JsxChildren`、`ImportAttributes`（`isListElement`/`isListTerminator`/`isInSomeParsingContext`）。
- deepclone 真实解析回填扩展到 **~107 个 case**（TS + 新增 TSX/JSON 表），覆盖本轮全部新节点。

**🔴 仍 DEFER（明确理由）—— JSDoc + reparser（含 `TestJSDocImportTypeParentChain`）**：
JSDoc 注释当前由 **scanner 作为 trivia 跳过**（`jsdoc_comments_are_skipped_as_trivia` 测试验证：含 `/** @type ... */` 的 JS 文件零诊断、语句结构正确），但 JSDoc 的**语义解析 + reparser**（`internal/parser/jsdoc.go` ≈1323 行 + `internal/parser/reparser.go` ≈623 行：JSDoc tag 解析、把 JSDoc 类型 reparse 成真实类型节点、`ReparsedClones` 跟踪、parent 链回挂、从 JSDoc import-type 收集 `Imports()`）是一个 ≈2000 行的独立子系统。鉴于 §8.6「每函数一测」要求 + 子系统规模，本轮未能在保证质量前提下完成，**整体 DEFER**。`TestJSDocImportTypeParentChain` 依赖该子系统，随之 DEFER。**理由**：(1) JSDoc/reparser 仅服务 **JS-with-JSDoc-types**；本轮要解锁的 binder/astnav/module 主路径是 **TS**，其 grammar 已完整；(2) JSDoc 注释已被正确跳过，不破坏任何解析；(3) 该子系统规模独立，宜作为专门 round 移植。其余微项（unclosed-JSX-tag restructure、`<a/><b/>` binary-comma 恢复、dynamic-import/require 调用的 `Imports` 收集、ambient module augmentation、pragmas/commentDirectives、top-level-await reparse、`MissingDeclaration` 节点）同 DEFER，均 `// DEFER(phase-3/4)` 标注。

**完整度评估**：核心递归下降 grammar（语句 / 声明 / 表达式 / 类型 / JSX / JSON / 装饰器 / import 属性 / 模块引用）**已完整**，足以支撑 binder/astnav/module。唯一成体系的缺口是 JSDoc+reparser 语义层（JS-only）。

## 本轮实现进度（vertical slice，red→green）

已落地并全绿（`cargo test -p tsgo_parser` + clippy `-D warnings` + gate C1–C8 对本 crate 全绿）的**垂直切片**，逐行为推进：

- 入口/状态：`parse_source_file`、`parse_isolated_entity_name`、`initialize_state`、`parse_source_file_worker`（子集）。
- token/诊断：`next_token`(+escape 检查)/`next_token_without_check`/`node_pos`/`has_preceding_line_break`/`parse_error_at*`（去重）+ scanner `scan_errors` sink（`drain_scan_errors`）。
- token 助手：`parse_optional`/`parse_expected`/`parse_expected_token`/`parse_token_node`/`parse_optional_token`。
- 列表/收尾：`parse_list_index`/`parse_delimited_list`（子集上下文）/`new_node_list`/`finish_node`/`finish_node_with_end`/`override_parent_in_immediate_children`。
- 语句：`parse_statement`（`;` → `parse_empty_statement`；其余 → `parse_expression_or_labeled_statement` 的 expression-statement 半）。
- 表达式链（1:1 子集）：`parse_expression`→`parse_assignment_expression_or_higher`（**无 yield/arrow**，DEFER）→`parse_binary_expression_or_higher`/`parse_binary_expression_rest`（含 `re_scan_greater_than_token` + `**` 右结合；`as/satisfies` 为 `todo!` DEFER）→`parse_conditional_expression_rest`→`parse_unary_expression_or_higher`/`parse_update_expression`/`parse_simple_unary_expression`/`parse_prefix_unary_expression`→`parse_left_hand_side_expression_or_higher`/`parse_member_expression_or_higher`/`parse_member_expression_rest`(`.`/`[`)/`parse_call_expression_rest`(`(`)/`parse_element_access_expression_rest`/`parse_argument_list`/`parse_spread_element`→`parse_primary_expression`(literal/keyword/paren/array/identifier)/`parse_parenthesized_expression`/`parse_array_literal_expression`/`parse_keyword_expression`/`parse_literal_expression`。
- 标识符/实体名：`parse_entity_name`/`parse_right_side_of_dot`(子集)/`parse_identifier(_name)`/`create_identifier_with_diagnostic`/`create_missing_identifier`/`new_identifier`。
- 谓词/上下文：`is_start_of_statement`/`is_start_of_expression`/`is_start_of_left_hand_side_expression`/`is_identifier`/`is_binary_operator`/`is_list_element`/`is_list_terminator`/`can_parse_semicolon`/`try_parse_semicolon`/`set_context_flags`/`in_*_context`/`re_scan_greater_than_token`。
- `types.rs`（`ParseFlags`）、`utilities.rs`（`get_language_variant`/`token_is_identifier_or_keyword(_or_greater_than)`/`is_keyword_or_punctuation`/`is_jsdoc_like_text`）全绿。
- deepclone 回填：在 `internal/parser/deepclone_test.rs` 用真实解析 + `arena.deep_clone_node` 跑 Go `TestDeepCloneNodeSanityCheck` 的 BFS 不变量（28 个代表性 case，覆盖本切片能解析的节点种类）。

**仍 DEFER(phase-3)**（本轮未触达，留待后续切片，均按 `// DEFER` + `// blocked-by:` 标注）：
arrow function（`mark`/`rewind`/`look_ahead` 投机随之落地）、JSON（`parse_json_text`/`validateJson*`）、类型节点（`as`/`satisfies`/泛型实参回扫/`parseType`）、声明/语句全族（var/function/class/if/for/…）、object/function/class/new/template/regex/JSX primary、JSDoc（`jsdoc.rs`）/reparse（`reparser.rs`）/references（`references.rs`）、pragma/外部模块指示器/顶层 await reparse、`finish_source_file`。
其中 `TestJSDocImportTypeParentChain`（tests.md 唯一可移植 Go 单测）依赖 JSDoc reparse，DEFER 到 JSDoc/reparse 切片。

### 本轮 ast（`tsgo_ast`）的**附加**（additive，未破坏既有公开 API）

- `NodeData` 新增 variant + 构造器 + `for_each_child` + `visit_each_child`：`EmptyStatement`、`ConditionalExpression`（`ConditionalExpressionData`）、`SourceFile`（`Box<SourceFileData>`：statements/eof/file_name/script_kind/language_variant/is_declaration_file/external_module_indicator）。
- `NodeArena::data_mut`（in-place 改 SourceFile flag）。
- `utilities.rs` 新增纯谓词：`node_is_missing`/`node_is_present`/`is_keyword`/`is_keyword_kind`/`is_punctuation_kind`/`is_token_kind`/`is_modifier_kind`/`is_assignment_operator`/`is_left_hand_side_expression_kind`。

## 实现进度（Round 6ad-fix — 动态 `import(...)` 表达式：消除挂死）

修复了一个 **parser 无限循环（挂死）** bug：动态 `import("m")`（及 `const p = import("m");`）在**表达式位置**的 `import` 关键字无法被消费，导致 token 不前进、`SourceElements` 列表循环永不终止。

**根因**：`parse_left_hand_side_expression_or_higher` 仅处理 `import.meta`（`import` + lookahead `.`），其余 `import` 落到 `parse_member_expression_or_higher` → `parse_primary_expression`。后者对 `ImportKeyword` 命中 `_ => parse_identifier_with_diagnostic(...)` 分支，在非标识符 token 上构造**缺失标识符**且**不 `next_token`**，故 token 停在 `ImportKeyword` → 无限循环。确认完全位于 `internal/parser/lib.rs`。

**修复（additive，红→绿）**：在 `parse_left_hand_side_expression_or_higher` 内、`import.meta` 分支**之前**，按 Go `parseLeftHandSideExpressionOrHigher` 加入动态 import 分支——`import` + `look_ahead(next_token_is_open_paren_or_less_than)`（`(`/`<`）→ `parse_keyword_expression()`（构造 `KeywordExpression{ kind: ImportKeyword }` 并前进），随后 `parse_call_expression_rest` 消费实参表，得到 `CallExpression{ expression: ImportKeyword, arguments }`——正是 6ad transformer 期望的节点形状。`import.meta` 分支保持不变（`MetaProperty{ ImportKeyword, "meta" }`，已在 Round 3 落地，本轮加守卫测试）。未改动任何公开签名（`parse_source_file`/`ParseResult` 不变），`tsgo_ast` 未触碰（`new_keyword_expression` 已支持 `ImportKeyword`，见 `IsKeywordExpressionKind`）。

红→绿切片：
1. **RED**：`parse_dynamic_import_call`（`import("m");`）—— 实测**编译通过但运行挂死**（`test ... parse_dynamic_import_call ...` 启动后永不返回，perl alarm 看门狗确认）。**GREEN**：加动态 import 分支 → `CallExpression{ ImportKeyword, [StringLiteral] }`，0 诊断，测试 0.00s 通过。
2. **守卫**：`parse_import_keyword_statement_vs_call` —— `import x from "m";` 仍为 `ImportDeclaration`（import 语句路径不受 lookahead 影响，因其下一 token 是标识符而非 `(`/`<`/`.`）；`import("m");` 为 `CallExpression`。`import.meta` 守卫沿用既有 `parse_meta_properties`。

**仍 DEFER**（已 `// DEFER` + `// blocked-by:`）：动态 import 的 import attributes/`with {}`（`import("m", { with: ... })` 调用形态）、`sourceFlags |= PossiblyContainsDynamicImport`（blocked-by: finishSourceFile / source-flags 跟踪）、`import.defer(...)` 阶段修饰符的 dynamic-import 标志。

## 与 Go 的已知偏离（divergence）

- **`testdata` 不是子包**：用户清单提到"parser 有 3 个子目录"。实测 `internal/parser/` 下唯一目录是 `testdata/`，其内嵌套 `testdata/fuzz/FuzzParser/<10 个语料文件>`（被 `find -type d` 数成 3 个层级目录）。它们是 **Go fuzz 语料**，**无 `.go` 源**，故 parser **没有源码子包**，不产生子 crate/子 module。fuzz 语料整体推迟到 P10。
- **`missingListNodes` 指针相等哨兵**：Go 用"底层数组地址相等"区分 missing/empty 列表（`isMissingNodeList` 比较 `&missingListNodes[0]`）。Rust 无法靠地址相等（且不安全）。决策：`NodeList` 加 `is_missing: bool` 字段，或 `Option<NodeList>` 区分。**须在执行期确认所有 `isMissingNodeList` 调用点都改判此 flag**。
- **`node.Parent` → `arena[id].parent`**：见 PORTING §5。`finishNode` 回填子 parent 用 `NodeId` 写 arena，不用 Rust 引用。
- **`sync.Pool` → 暂不池化或 `thread_local`**：第一版可每次新建 Parser（`// PERF(port): parser pool`）。
- **`lookAhead`/`tryParse` 传闭包 + `&mut self`**：Go 传 `func(p *Parser)`，Rust 用 `impl FnOnce(&mut Parser)->R`。投机回滚靠 `ParserState` 截断诊断/jsdoc/reparse 列表（语义 1:1）。
- **`expressions any`（JSON）→ `enum OneOrMany`**：判别联合替代 `any` 断言。
- **`setParentFromContext` 复用闭包**：Rust 直接在 `finish_node` 内联遍历，无需常驻闭包字段（或保留以对齐结构）。
- **返回 `ParseResult{arena, source_file, diagnostics}` 而非 `*ast.SourceFile`**：Go 的 SourceFile 由 GC 图持有，节点随返回值存活；Rust 的 arena 拥有全部节点，故 `parse_source_file` 必须连 arena 一起移交。诊断同理放进 `ParseResult`。
- **`ast.Diagnostic` 暂用 parser 本地 `Diagnostic{loc, message:&'static Message, args}`**：完整 `ast.Diagnostic`（message chain/related info/file 反指针/`DiagnosticsCollection`）尚未在 P2 落地，本轮以最小载体支撑「有/无诊断 + 消息码」断言；待 `ast/diagnostic.rs` 落地后切换。
- **scanner 错误回调 → `Rc<RefCell<Vec<ScanError>>>` sink**：Go `SetOnError(p.scanError)` 直接回调改 `p.diagnostics`；安全 Rust 下 scanner 被 parser 拥有，闭包不能再借 `&mut Parser`，故 scanner 把原始 scan error 推入共享 sink，parser 每次 `scan()` 后 `drain_scan_errors` 经 `parse_error_at_range` 注入（保持去重/顺序）。本切片无 scan error，行为等价。
- **`arrow` / `yield` 投机（Round 3 已落地）**：`parse_assignment_expression_or_higher_worker` 现含 yield 检测 + 括号/简单/async 箭头三态投机（`mark`/`rewind`/`look_ahead`）。
- **箭头投机 `parseParametersWorker` 简化**：当前 `parse_parameters_worker` 始终成功（不实现 Go 的 `parseParameterEx(allowAmbiguity=false)` 返回 nil 与 `parseDelimitedList` 的 nil 传播）；括号箭头的消歧依赖签名后的 `=>`/`{` 检查 + `parse_expected(CloseParen)` 失败回滚，对常见用例（`(x)`、`(a,b)`、`(a)=>`、`(a:T):U=>` 等）与 Go 一致。**已知边界**：形如 `({ m() {} })`（含方法/访问器的括号-对象-字面量）箭头投机可能误判——故对象字面量测试改用赋值 RHS 形态（`x = {...}`，不触发 paren-arrow 投机）。DEFER(phase-3) 精修。
- **表达式中泛型实参 `f<T>()` DEFER**：`parse_member_expression_rest`/`parse_call_expression_rest` 未实现 `tryParseTypeArgumentsInExpression`（消歧 `a<b>c` vs `f<T>()` 需复杂回扫），故 `f<T>()` 暂解析为 `f < T > ()`。常规 `f()`/`a?.b()`/标签模板均工作。`tryReparseOptionalChain`（`!` 后可选链传播）亦 DEFER。

## 转交 / 推迟（DEFER）

- **`BenchmarkParse` / `FuzzParser`**：性能基准 + 模糊测试，`// DEFER(phase-10)`（需 testdata/TypeScript submodule）。
- **完整产生式覆盖的正确性 gate**：靠 P10 conformance/fourslash baseline（4250 fourslash + 294MB testdata 对拍）。本轮 tests.md 只收口可移植的 `TestJSDocImportTypeParentChain` + 少量行为级。
- **诊断消息逐条文本对齐**：依赖 `tsgo_diagnostics`（已在 P1/P2），但完整诊断 parity 在 P10。

## Round 7 — `lib.rs` 公开 API 注释 + §8.6 覆盖（2026-05-31）

- [x] §8.6 审计 crate 根公开面：`parse_source_file`、`parse_isolated_entity_name`、`SourceFileParseOptions`、`Diagnostic`（含 `pos()`）、`ParseResult`、`pub use ParseFlags`、`pub mod types|utilities`。
- [x] 新增 5 条行为级单测（`declaration_file_name_*`、`diagnostic_pos_*`、`diagnostic_deduplication_*`、`parse_source_file_unknown_script_kind_panics`）——全部为 **characterization/coverage**，无 Go 分歧 RED。
- [x] Rustdoc 增强：`SourceFileParseOptions` / `Diagnostic` / `Diagnostic::pos()` / `ParseResult` 补 `# Examples`；`Diagnostic::pos()` 补 `// Go:` 锚；`ParseResult` 补 `// Go:` 锚。doctest 3→7。
- [ ] 完整 `ast.Diagnostic` 替换 parser 本地 `Diagnostic` — DEFER(blocked-by: ast diagnostic 子系统)。
