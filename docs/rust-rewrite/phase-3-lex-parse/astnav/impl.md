# astnav: 实现方案（impl.md）

**crate**：`tsgo_astnav`　**目标**：在已解析的 `ast.SourceFile` 上做**基于位置的 AST 导航**——给定字节位置，找到所在/相邻 token 或节点（含按需用 scanner 合成 trivia 中不存在于 AST 的 token），是语言服务（hover、go-to-def、补全、签名帮助等）的底层定位工具。
**依赖（crate）**：`tsgo_ast` `tsgo_scanner` `tsgo_core`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/astnav/`（1 个非测试文件：`tokens.go` 26KB）

## 这个包是什么（业务说明）

parser 产出的 AST **只含语法节点**：标点/关键字等"平凡 token"通常不作为独立节点存在（它们隐含在父节点的 `[pos,end)` 区间里）。但语言服务经常需要"光标在第 N 个字节处，那是哪个 token？"——可能落在某个真实 AST 节点上，也可能落在两个节点之间的标点/空白/注释里。astnav 就是把"位置 → token/节点"这件事做对：

- 在 AST 上**逐层下钻**（`visitNode` 顺序遍历子节点、`visitNodeList` 对列表二分查找）找到包含目标位置的最深节点；
- 若最深处不是一个 token 节点，就**用 scanner 从已知边界 `left` 处重新扫描**，合成出位置处的 token，并通过 `sourceFile.GetOrCreateToken(...)` **缓存**（保证同位置多次查询返回同一对象——指针相等）；
- 处理 **JSDoc**（JSDoc 子树不含 trivia token，需特判）、**JSX**（`<<` 在 JSX child 里要 `ReScanJsxToken`）、**reparsed 节点**（JS 文件 JSDoc 重解析出的合成节点要跳过，`NodeFlagsReparsed`）。

对外 5 个入口：`GetTokenAtPosition`（含前导 trivia）、`GetTouchingToken` / `GetTouchingPropertyName`（不含前导 trivia，后者带"属性名/关键字/私有标识符"过滤回调）、`FindPrecedingToken(Ex)`（找位置左侧最近 token）、`FindNextToken`（给定 token 找下一个）、`FindChildOfKind`（在容器内找指定 Kind 的子节点/token）。

为什么在 P3：astnav 依赖 ast（P2）、scanner+parser（P3），自身是 P7 语言服务（`ls`）的前置工具，放 P3 末尾随 parser 落地。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包是**只读遍历 + scanner 合成**，无新建持久结构。特有决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `*ast.Node`（遍历游标、返回值） | `NodeId`（P2 arena 索引）/ `Option<NodeId>` | 全程用 `NodeId`；`nil` → `None`。返回的"合成 token"也是 arena 里的 `NodeId`（经 `GetOrCreateToken` 落入 SourceFile 的 token 缓存）。 |
| 闭包 visitor（`visitNode func(*Node, *NodeVisitor)*Node`） | `impl FnMut(NodeId, &NodeVisitor)->NodeId` 闭包 | Go 用 `ast.NewNodeVisitor` + hooks 驱动 `VisitEachChild`。Rust 复用 P2 的 `NodeVisitor` 抽象；闭包捕获 `&mut` 本地状态（`next`/`left`/`prevSubtree`…）——**借用注意**：这些是函数内局部变量，用 `Cell`/可变捕获即可，无需全局。 |
| `sourceFile.GetOrCreateToken(kind,fullStart,end,parent,flags)` | `source_file.get_or_create_token(...) -> NodeId`（P2 ast 提供） | **token 缓存命门**：同 `(pos,kind,...)` 必须返回同一 `NodeId`，保证 `GetTokenAtPosition(f,0)==GetTokenAtPosition(f,0)`（见测试"pointer equality"）。缓存由 SourceFile 持有（P2 实现），astnav 只调。 |
| `core.BinarySearchUniqueFunc(nodes, cmp)` | `core::binary_search_unique_by(slice, cmp)`（P1 core） | 列表二分。`cmp` 返回三态 `-1/0/1`。本包大量用它在 `NodeList.Nodes` 上定位。 |
| `node.Flags&ast.NodeFlagsReparsed != 0` | `node.flags.contains(NodeFlags::REPARSED)` | 跳过 JS 文件 JSDoc reparse 合成节点（它们 pos/end 与真实节点重叠，会干扰二分）。**遍历各处都要过滤**。 |
| `scanner.GetScannerForSourceFile(sf, pos)` | `scanner::get_scanner_for_source_file(sf, pos) -> Scanner` | 在 `left` 边界起一台 scanner 扫合成 token。 |
| `scanNavigationToken(s, containingNode)` | `fn scan_navigation_token(s, containing)->Kind` | JSX child 里 `<<` 要 `ReScanJsxToken(true)`，其余直接 `s.Token()`。 |
| 多个 `find`/递归闭包（`FindPrecedingTokenEx` 内 `var find func(...)`） | 内部 `fn` 或递归闭包 | Go 用闭包递归（捕获 `sourceFile`/`position`/`excludeJSDoc`）。Rust 优先抽成带参数的内部 `fn`（避免自引用闭包的生命周期麻烦）。 |
| panic（`"did not expect ..."` / `"Expected result to be non-whitespace"`） | `panic!`/`unreachable!` | 1:1 保留断言语义（这些 panic 在测试里被 `recover` 兜，见 FindNextToken 测试）。 |

### 遍历不变量（要点）

- `getTokenAtPosition` 维护 `left`（可返回 token 的下界，最终是 scanner 起点）、`nodeAfterLeft`（推进 `left` 后第一个被访问节点，限制 scanner 扫描上界）、`next`（下次下钻的子节点）、`prevSubtree`（end 恰等于 position 的子树，配合 `includePrecedingTokenAtEndPosition`）。这些是 `&mut` 局部状态，Rust 用结构体或多个 `Cell` 持有，传给 visitor 闭包。
- "包含"判定：`position < node.End()`，但**文件末尾节点把 end 视作包含**（EOF token、到 EOF 的未闭合 JSDoc）。逐行对齐 `testNode`。
- scanner 扫描区间严格限制在 `[left, end)`（`end` = `current.End()` 或 `nodeAfterLeft.Pos()`），防止跨过下一个 AST 节点。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/astnav/tokens.go` | `internal/astnav/lib.rs`（crate 根，basename==目录名） | 全部内容：5 个公开入口 + `getTokenAtPosition` 核心 + `FindPrecedingTokenEx`/`findRightmostValidToken`/`FindNextToken`/`FindChildOfKind` + `VisitEachChildAndJSDoc`/`getNodeVisitor`/`findRightmostNode`/`getPosition`/`GetStartOfNode`/`isValidPrecedingNode`/`shouldSkipChild`/`scanNavigationToken`/`shouldRescanLessThanLessThanToken`。 |

## 依赖白名单（本包新增的 crate）

- 无新增。仅用 `tsgo_ast`（NodeVisitor/NodeFlags/GetOrCreateToken/各 `IsXxx` 判定）、`tsgo_scanner`（GetScannerForSourceFile/ReScanJsxToken/GetTokenPosOfNode）、`tsgo_core`（BinarySearchUniqueFunc/IfElse/Filter/TextRange）。

## 实现 TODO（逐文件 / 逐函数，可勾选）

> 顺序按 TDD 推进序：先公共遍历器与小工具，再 `getTokenAtPosition`，再 preceding/next/childOfKind。

### `lib.rs` — 公共遍历与小工具（Go: `tokens.go`）

- [ ] `fn should_rescan_less_than_less_than_token(s, containing, token)->bool`　`// Go: tokens.go:shouldRescanLessThanLessThanToken`
- [ ] `fn scan_navigation_token(s, containing)->Kind`　`// Go: tokens.go:scanNavigationToken`
- [ ] `pub fn visit_each_child_and_jsdoc(node, sf, visit_node, visit_nodes)`（先遍历 JSDoc 再 `VisitEachChild`）　`// Go: tokens.go:VisitEachChildAndJSDoc`
- [ ] `fn get_node_visitor(visit_node, visit_nodes)->NodeVisitor`（包裹：跳过 `IsJSDocSingleCommentNodeComment/List`；hook VisitNode/VisitToken/VisitNodes/VisitModifiers）　`// Go: tokens.go:getNodeVisitor`
- [ ] `fn get_position(node, sf, allow_in_leading_trivia)->i32`（true→`Pos()`；false→`GetTokenPosOfNode`）　`// Go: tokens.go:getPosition`
- [ ] `pub fn get_start_of_node(node, sf, include_jsdoc)->i32`（=`scanner::get_token_pos_of_node`）　`// Go: tokens.go:GetStartOfNode`
- [ ] `fn is_valid_preceding_node(node, sf)->bool`（非空宽度、非纯空白 JsxText；EOF 仅当有 JSDoc）　`// Go: tokens.go:isValidPrecedingNode`
- [ ] `fn should_skip_child(node)->bool`（JSDoc/JSDocText/JSDocTypeLiteral/JSDocSignature/link/tag）　`// Go: tokens.go:shouldSkipChild`
- [ ] `fn find_rightmost_node(node)->NodeId`（一路取最后可见子节点）　`// Go: tokens.go:findRightmostNode`
- [ ] 常量 `COMPARISON_LESS_THAN/EQUAL_TO/GREATER_THAN`　`// Go: tokens.go:comparison*`

### `lib.rs` — 公开入口（Go: `tokens.go`）

- [ ] `pub fn get_touching_property_name(sf, pos)->NodeId`（`allow_in_leading_trivia=false` + 过滤 `IsPropertyNameLiteral||IsKeywordKind||IsPrivateIdentifier`）　`// Go: tokens.go:GetTouchingPropertyName`
- [ ] `pub fn get_touching_token(sf, pos)->NodeId`（false，无过滤）　`// Go: tokens.go:GetTouchingToken`
- [ ] `pub fn get_token_at_position(sf, pos)->NodeId`（true，无过滤）　`// Go: tokens.go:GetTokenAtPosition`

### `lib.rs` — 核心 `getTokenAtPosition`（Go: `tokens.go`）

- [ ] `fn get_token_at_position(sf, pos, allow_in_leading_trivia, include_preceding_at_end: Option<impl Fn(NodeId)->bool>)->NodeId`　`// Go: tokens.go:getTokenAtPosition`
  - [ ] `test_node` 闭包：包含判定 + `prevSubtree` 设置（end==position 且回调存在且非 reparsed）
  - [ ] `visit_node` 闭包：推进 `left`/`nodeAfterLeft`/`next`（跳 reparsed；JSDoc 不推 left）
  - [ ] `visit_node_list` 闭包：末尾==position 特判、`<=position` 推进、`<=pos<end` 二分（含 reparsed 二次过滤搜索）
  - [ ] 主循环：`VisitEachChildAndJSDoc` → prevSubtree 检查（`FindPrecedingTokenEx`）→ 无 next 时用 scanner 扫 `[left,end)` 合成 token（`GetOrCreateToken`）→ 下钻 `current=next`
  - [ ] scanner 路径里 `token==Identifier || !IsTokenKind`：JSDoc 返回 current，否则 panic（保留断言）

### `lib.rs` — preceding / rightmost-valid（Go: `tokens.go`）

- [ ] `pub fn find_preceding_token(sf, pos)->NodeId`（=`FindPrecedingTokenEx(sf,pos,None,false)`）　`// Go: tokens.go:FindPrecedingToken`
- [ ] `pub fn find_preceding_token_ex(sf, pos, start_node: Option<NodeId>, exclude_jsdoc)->NodeId`（递归 `find`：二分定位 foundChild、look-in-previous-child、JSDoc 分支、childless/trailing 处理；结尾断言非纯空白 JsxText）　`// Go: tokens.go:FindPrecedingTokenEx`
- [ ] `fn find_rightmost_valid_token(end_pos, sf, containing, position, exclude_jsdoc)->NodeId`（三情形：rightmostValidNode 递归 / 未访问尾部 token 扫描 / childless 节点；含 `shouldVisitNode`/`rightmostVisitedNodes`）　`// Go: tokens.go:findRightmostValidToken`

### `lib.rs` — next / child-of-kind（Go: `tokens.go`）

- [ ] `pub fn find_next_token(previous_token, parent, file)->NodeId`（递归找：token 起点==prev.End() 返回 / 含 prev 的节点下钻 / scanner 扫下一 token，`tokenFullStart==prev.End()` 校验）　`// Go: tokens.go:FindNextToken`
- [ ] `pub fn find_child_of_kind(containing, kind, sf)->Option<NodeId>`（遍历子节点间隙 + 尾部用 scanner 找指定 Kind）　`// Go: tokens.go:FindChildOfKind`

### Cargo / crate 接线

- [ ] `internal/astnav/Cargo.toml`（`name = "tsgo_astnav"` + path deps：ast/scanner/core）
- [ ] 根 `Cargo.toml` workspace members 追加 `internal/astnav`
- [ ] `lib.rs` re-export 5 个公开入口 + `VisitEachChildAndJSDoc`/`GetStartOfNode`

## TDD 推进顺序（tracer bullet → 增量）

1. **`VisitEachChildAndJSDoc` + `getNodeVisitor`**：先打通遍历器（依赖 P2 NodeVisitor），用简单源验证子节点访问顺序。
2. **`get_token_at_position` 最小路径**：纯 AST 命中（光标落在标识符上），不触发 scanner。配 tests.md 的 `pointer equality`（同位置返回同 NodeId）+ `JSDoc type assertion`（不 panic）。
3. **scanner 合成路径**：光标落在两 token 间的标点上 → 走 scanner 合成 + `GetOrCreateToken`。
4. **`find_preceding_token` + `find_rightmost_valid_token`**：实现 `TestUnitFindPrecedingToken` 两例（"after dot in jsdoc"、"after comma in parameter list"）。
5. **`find_next_token` / `find_child_of_kind`**：补齐。
6. baseline 对拍（GetTokenAtPosition/GetTouchingPropertyName/FindPrecedingToken/FindNextToken）→ 推迟 P10（需 TS submodule + Node.js）。

## 与 Go 的已知偏离（divergence）

- **`*ast.Node` → `NodeId`**：所有游标/返回值用索引；`nil`→`None`。合成 token 同样是 arena `NodeId`（经 SourceFile token 缓存）。
- **递归闭包 → 内部 `fn`**：`FindPrecedingTokenEx`/`findRightmostValidToken`/`FindNextToken` 的 `var find func(...)` 自引用闭包在 Rust 难表达，改为带显式参数的内部 `fn`（捕获改传参：`sf`/`position`/`exclude_jsdoc`）。
- **visitor 闭包捕获可变局部**：`next`/`left`/`prevSubtree`/`foundChild`/`prevChild` 等用 `&mut` 捕获或 `Cell`；保证与 Go 同序更新。
- **token 缓存语义靠 P2**：`GetOrCreateToken` 的指针相等（`pointer equality` 测试）由 ast crate 的 SourceFile 缓存保证；astnav 不自建缓存。本包测试该项时需 P2 缓存就位。
- **panic 保留**：合成路径与结尾断言的 panic 1:1 保留（`FindNextToken` baseline 测试靠 `recover` 兜，Rust 侧对应 `catch_unwind` 或在 baseline 生成器里跳过）。

## 转交 / 推迟（DEFER）

- **baseline 对拍测试**（`TestGetTokenAtPosition`/`TestGetTouchingPropertyName`/`TestFindPrecedingToken`/`TestFindNextToken` 的 baseline 与 go-json 子测试）：依赖 `repo.TypeScriptSubmodulePath()` + `jstest`(Node.js 跑真实 `typescript`) + `testutil/baseline` 框架。`// DEFER(phase-10)`。
- **`TestMain`（baseline.Track）**：测试设施，随 P10 baseline 框架移植。
