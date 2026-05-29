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

- [ ] `enum ParsingContext`（25 项，含 `Count`）、`struct ParsingContexts`（位集）　`// Go: parser.go:ParsingContext`
- [ ] `struct Parser`（全部字段，见类型映射）+ `new()/initialize_closures()`　`// Go: parser.go:Parser/newParser`
- [ ] `pub fn parse_source_file(opts, source_text, script_kind) -> SourceFile`（取 parser→`initialize_state`→`next_token`→JSON 或 worker）　`// Go: parser.go:ParseSourceFile`
- [ ] `pub fn parse_isolated_entity_name(text) -> Option<EntityName>`　`// Go: parser.go:ParseIsolatedEntityName`
- [ ] `fn initialize_state(opts, text, kind)`（建/重置 scanner、set contextFlags by kind、接 scanError）　`// Go: parser.go:initializeState`
- [ ] `fn parse_json_text(&mut self) -> SourceFile`（含 `OneOrMany` 错误恢复、`validateJsonValue/validateJsonObjectLiteral`）　`// Go: parser.go:parseJSONText/validateJson*`
- [ ] `fn parse_source_file_worker(&mut self) -> SourceFile`（`parseList(SourceElements)`→EOF→reparseList 合并→`finishSourceFile`→顶层 await reparse→`collectExternalModuleReferences`）　`// Go: parser.go:parseSourceFileWorker`
- [ ] `fn finish_source_file(...)`（commentDirectives/pragmas/diagnostics/CommonJS 指示器）　`// Go: parser.go:finishSourceFile`

### `lib.rs` — token 流与诊断（Go: `parser.go`）

- [ ] `fn next_token(&mut self)->Kind`（关键字含转义 → 报错）/ `next_token_without_check` / `next_token_jsdoc` / `next_jsdoc_comment_text_token`　`// Go: parser.go:nextToken/...`
- [ ] `fn node_pos(&self)->i32`（=`scanner.TokenFullStart()`）/ `has_preceding_line_break`　`// Go: parser.go:nodePos/hasPrecedingLineBreak`
- [ ] 诊断族：`scan_error/parse_error_at/parse_error_at_current_token/parse_error_at_range`（去重：同 pos 不重复报）　`// Go: parser.go:scanError/parseErrorAt*`
- [ ] `struct ParserState` + `fn mark()->ParserState` / `fn rewind(s)`（含 scanner state + 各 len 截断）　`// Go: parser.go:ParserState/mark`
- [ ] `fn look_ahead<R>(f)` / `fn try_parse<R>(f)`（投机；lookahead 必回滚，tryParse 成功保留）　`// Go: parser.go:lookAhead/tryParse`
- [ ] `fn parse_optional(token)->bool` / `parse_expected(kind)->bool` / `parse_expected_token(kind)->NodeId` / `parse_token_node()->NodeId`　`// Go: parser.go:parseOptional/parseExpected/...`

### `lib.rs` — 列表与节点收尾（Go: `parser.go`）

- [ ] `fn parse_list(ctx, parse_element)->NodeList` / `parse_list_index` / `parse_delimited_list(ctx, parse_element)->NodeList`（分隔符、trailing comma、错误恢复）　`// Go: parser.go:parseList/parseDelimitedList`
- [ ] `fn create_missing_list()->NodeList`（`missingListNodes` 哨兵 → Rust 用 `is_missing` 标志）　`// Go: parser.go:createMissingList/isMissingNodeList`
- [ ] `fn new_node_list(loc, nodes)` / `new_modifier_list(loc, nodes)`　`// Go: parser.go:newNodeList/newModifierList`
- [ ] `fn finish_node(id, pos)->NodeId` / `finish_node_with_end(id, pos, end)->NodeId`（设 Loc/flags/parent 回填）　`// Go: parser.go:finishNode/finishNodeWithEnd`
- [ ] `fn override_parent_in_immediate_children(id)`（`for_each_child` 写 `arena[c].parent=id`）　`// Go: parser.go:overrideParentInImmediateChildren`

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

## 与 Go 的已知偏离（divergence）

- **`testdata` 不是子包**：用户清单提到"parser 有 3 个子目录"。实测 `internal/parser/` 下唯一目录是 `testdata/`，其内嵌套 `testdata/fuzz/FuzzParser/<10 个语料文件>`（被 `find -type d` 数成 3 个层级目录）。它们是 **Go fuzz 语料**，**无 `.go` 源**，故 parser **没有源码子包**，不产生子 crate/子 module。fuzz 语料整体推迟到 P10。
- **`missingListNodes` 指针相等哨兵**：Go 用"底层数组地址相等"区分 missing/empty 列表（`isMissingNodeList` 比较 `&missingListNodes[0]`）。Rust 无法靠地址相等（且不安全）。决策：`NodeList` 加 `is_missing: bool` 字段，或 `Option<NodeList>` 区分。**须在执行期确认所有 `isMissingNodeList` 调用点都改判此 flag**。
- **`node.Parent` → `arena[id].parent`**：见 PORTING §5。`finishNode` 回填子 parent 用 `NodeId` 写 arena，不用 Rust 引用。
- **`sync.Pool` → 暂不池化或 `thread_local`**：第一版可每次新建 Parser（`// PERF(port): parser pool`）。
- **`lookAhead`/`tryParse` 传闭包 + `&mut self`**：Go 传 `func(p *Parser)`，Rust 用 `impl FnOnce(&mut Parser)->R`。投机回滚靠 `ParserState` 截断诊断/jsdoc/reparse 列表（语义 1:1）。
- **`expressions any`（JSON）→ `enum OneOrMany`**：判别联合替代 `any` 断言。
- **`setParentFromContext` 复用闭包**：Rust 直接在 `finish_node` 内联遍历，无需常驻闭包字段（或保留以对齐结构）。

## 转交 / 推迟（DEFER）

- **`BenchmarkParse` / `FuzzParser`**：性能基准 + 模糊测试，`// DEFER(phase-10)`（需 testdata/TypeScript submodule）。
- **完整产生式覆盖的正确性 gate**：靠 P10 conformance/fourslash baseline（4250 fourslash + 294MB testdata 对拍）。本轮 tests.md 只收口可移植的 `TestJSDocImportTypeParentChain` + 少量行为级。
- **诊断消息逐条文本对齐**：依赖 `tsgo_diagnostics`（已在 P1/P2），但完整诊断 parity 在 P10。
