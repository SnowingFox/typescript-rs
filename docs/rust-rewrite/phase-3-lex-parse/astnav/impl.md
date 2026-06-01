# astnav: 实现方案（impl.md）

**crate**：`tsgo_astnav`　**目标**：在已解析的 `ast.SourceFile` 上做**基于位置的 AST 导航**——给定字节位置，找到所在/相邻 token 或节点（含按需用 scanner 合成 trivia 中不存在于 AST 的 token），是语言服务（hover、go-to-def、补全、签名帮助等）的底层定位工具。
**依赖（crate）**：`tsgo_ast` `tsgo_scanner` `tsgo_core`（镜像 Go import 边，path 依赖）
**Go 源**：`internal/astnav/`（1 个非测试文件：`tokens.go` 26KB）

## 实施状态（P3 wave 3）：**IMPLEMENTED** — 全部导航入口已落地（JSDoc 路径除外）

前一轮报告 BLOCKED 的根因是「无 scanner `GetScannerForSourceFile`/`GetTokenPosOfNode`，且无 SourceFile token 缓存」。本轮在**不修改 ast/scanner** 的前提下，把这些依赖**就地在 astnav 内解决**：

- **本地导航上下文 `SourceFile`**：本 crate 自带的 `pub struct SourceFile { arena, root, text, language_variant, eof_token, token_cache }`，替代 Go 的 `*ast.SourceFile`（后者同时持有源文本与 token 缓存）。`SourceFile::new(arena, root, text)` 从 `tsgo_parser::ParseResult` 构造。
- **token 缓存（指针相等）就地实现**：`SourceFile.get_or_create_token(kind,pos,end,parent)`，键 `(parent,pos,end)`（镜像 Go `TokenCacheKey`），同位置多次查询返回同一 `NodeId` —— `pointer equality` 测试绿。
- **scanner 就地驱动**：私有 `get_scanner_for_source_file(text, lv, pos)` 用 `Scanner::new()/set_text/set_language_variant/reset_pos/scan()` 复刻 Go `GetScannerForSourceFile`；`get_token_pos_of_node` 在 astnav 内本地复刻（背书 `GetStartOfNode`）。
- **遍历用 `for_each_child`**：Go 的 `NodeVisitor` hook（区分单节点 vs NodeList + 二分查找）在 ast 中不存在；本轮把两类 hook **拍平成单一有序子节点流**（`visit_each_child_and_jsdoc`），调用方用**线性扫描**替代「对 NodeList 的二分查找」——对**排好序的子节点**结果一致（见下「与 Go 的已知偏离」）。
- **谓词本地实现**：`is_jsdoc_kind`/`is_jsdoc_node`/`is_non_whitespace_token`/`is_whitespace_only_jsx_text`/`is_jsx_child`/`is_property_name_literal`/`is_private_identifier`/`find_last_visible_node`/`should_skip_child` 均在 astnav 内用 `Kind` + 节点数据本地实现（`is_token_kind`/`is_keyword_kind`/`node_is_missing` 复用 `tsgo_ast::utilities`）。

**JSDoc 仍 DEFER**：parser 尚未移植 JSDoc reparser，树中无 JSDoc-kind 节点、节点无缓存 JSDoc。JSDoc 相关分支**按结构 1:1 移植**（保持与 Go 同形），但运行时恒为惰性（`node_jsdoc` 恒返回空），均带 `// DEFER(phase-3)` + `// blocked-by: JSDoc reparser`。
**`get_node_visitor` 未单独移植**：它是 `NodeVisitor` hook 的包装器；本轮 hook 被拍平，故其语义（跳过 `IsJSDocSingleCommentNode*`）并入 `visit_each_child_and_jsdoc`，在无 JSDoc 的树上为 no-op。

自检：`cargo test -p tsgo_astnav`（32 lib + 17 doctest）全绿；`cargo clippy -p tsgo_astnav --all-targets -- -D warnings` clean；astnav 自身满足 gate-code C1–C8（仓库级 gate 因并行的 binder/module crate 缺模块文件而红，与 astnav 无关）。

## 实施状态（P7 enablement round）：**SHARED-BORROW 导航面已落地（additive）**

> 这是解锁整条 P7 语言服务河流的基石轮。LS 持有 program 的节点为**共享** `&NodeArena` / `Rc<NodeArena>`（与 checker 共享同一 arena），无法交出独占 `&mut`，而旧 `astnav::SourceFile` 既**按值持有 arena**、又因 `get_or_create_token` 把按需合成的 token 写进那块 arena 而要求 `&mut`，故 LS 用不了。本轮在**完全 additive**前提下加了 `&self` 的共享借用面，并据此重启了 `lsutil` 被阻塞的 helper。

**设计（如何调和「合成 token」与「共享借用」）：**

- **泛型导航引擎 `NavEngine<A: Borrow<NodeArena>>`**：整套算法挪进一个按「如何借 arena」泛型化的上下文。`pub type SourceFile = NavEngine<NodeArena>`（原 owned API，别名，签名不变）、`pub type NavSourceFile<'a> = NavEngine<&'a NodeArena>`（借用）、`pub type RcSourceFile = NavEngine<Rc<NodeArena>>`（共享）。构造器：`SourceFile::new`（原）、`NavSourceFile::from_borrowed_arena`、`RcSourceFile::from_rc_arena`。
- **合成 token 侧存储（内部可变）**：按需合成的 token 不再写进被借用的 arena，而是落入上下文自带的 `RefCell<SynthesizedTokenStore>`，故所有公开查询取 `&self`（共享），缓存在内部变更。`get_or_create_token(&self, ...)` 走 `borrow_mut`。
- **合成 id 高位打标（防碰撞）**：合成 token 的 `NodeId` 打上高位 `SYNTHESIZED_NODE_TAG = 1<<31`（真实 id 是 `nodes.len() as u32`，永不触及 2^31），与真实 arena id 形成不相交 id 空间（镜像 checker 给瞬态符号高位打标的 4x 做法）。`kind`/`pos`/`end`/`flags`/`node_is_missing`/`is_whitespace_only_jsx_text` 透明分发到「真实 arena」或「侧存储」。`for_each_child` 只在真实节点上调用（合成 token 永远是叶子）。
- **owned 与 shared 共享同一代码路径**：连原 owned `&mut` API 内部也只需 `&self`（`&mut` 仅为源码兼容保留）；所有核心算法函数（`token_at_position_core`/`fpt_ex`/`frvt_find`/`fnt_find`/`find_child_of_kind_core` 等）泛型化为 `fn ...<A: Borrow<NodeArena>>(file: &NavEngine<A>, ...)`，由 owned 自由函数（`&mut SourceFile`，签名原样保留）与 shared 方法（`&self`）共同委托。

**新增公开面（additive only）：** 类型别名 `NavSourceFile<'a>` / `RcSourceFile` + 构造器；`NavEngine<A>` 上的 `&self` 方法 `get_token_at_position`/`get_touching_token`/`get_touching_property_name`/`find_preceding_token(_ex)`/`find_next_token`/`find_child_of_kind` 与访问器 `kind`/`pos`/`end`/`root`/`text`/`arena`；自由函数 `get_start_of_node`/`visit_each_child_and_jsdoc` 放宽为泛型 `&NavEngine<A>`（原 `&SourceFile` 调用点不变）。**原 `&mut SourceFile` 自由函数签名与 `SourceFile`/`SourceFile::new` 一字未改**（checker/transformers/compiler 透传依赖不受影响——`cargo build --workspace --all-targets` 绿）。

**TDD 切片（红→绿）：**
1. `RcSourceFile::from_rc_arena(...).get_token_at_position` 与 owned `&mut` API 同输入返回同 token（parity）。RED：`RcSourceFile`/`NavSourceFile` 未声明（`E0433`）→ GREEN。
2. 合成 id 非碰撞 guard：合成 `;` 的查询不破坏真实节点查询；合成 id 带 tag 位、真实 id 无；剥掉 tag 位得到的真实 id 解析到不同节点。GREEN（设计即满足该不变量）。

自检（本轮）：`cargo test -p tsgo_astnav` → **35 lib + 26 doctest 全绿**（既有 `&mut` API 测试一条未改、全绿）；`cargo clippy -p tsgo_astnav --all-targets -- -D warnings` clean；`cargo fmt -p tsgo_astnav -- --check` clean。**解锁 P7：** `tsgo_ls_lsutil` 已据此重启 `IsCompletedNode`/`hasChildOfKind`/`PositionBelongsToNode` 与 `NodeIsASICandidate`/`PositionIsASICandidate`；`format` 的 `FormatDocument`/SmartIndenter（轮 2）与 `ls` 根仍 DEFER（后续轮次）。新增 scanner additive 助手 `get_ecma_line_of_position(text,pos)`。

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

- [x] `fn should_rescan_less_than_less_than_token(file, containing, token)->bool`　`// Go: tokens.go:shouldRescanLessThanLessThanToken`
- [x] `fn scan_navigation_token(scanner, file, containing)->Kind`　`// Go: tokens.go:scanNavigationToken`
- [x] `pub fn visit_each_child_and_jsdoc(file, node, visit)`（先遍历 JSDoc 再子节点；**拍平**单一子节点流，见偏离）　`// Go: tokens.go:VisitEachChildAndJSDoc`
- [x] `get_node_visitor`：**并入** `visit_each_child_and_jsdoc`（hook 拍平后无独立函数；其 `IsJSDocSingleCommentNode*` 跳过语义在无 JSDoc 树上为 no-op）。　`// Go: tokens.go:getNodeVisitor`
- [x] `fn get_position(file, node, allow_in_leading_trivia)->i32`（true→`pos()`；false→`get_token_pos_of_node`）　`// Go: tokens.go:getPosition`
- [x] `pub fn get_start_of_node(file, node, include_jsdoc)->i32`（=本地 `get_token_pos_of_node`）　`// Go: tokens.go:GetStartOfNode`
- [x] `fn is_valid_preceding_node(file, node)->bool`（非空宽度、非纯空白 JsxText；EOF 仅当有 JSDoc）　`// Go: tokens.go:isValidPrecedingNode`
- [x] `fn should_skip_child(file, node)->bool`（JSDoc/JSDocText/JSDocTypeLiteral/JSDocSignature/link/tag）　`// Go: tokens.go:shouldSkipChild`
- [x] `fn find_rightmost_node(file, node)->NodeId`（一路取最后可见子节点；Go 中亦为 dead code，`#[allow(dead_code)]`）　`// Go: tokens.go:findRightmostNode`
- [x] 常量 `comparison*`：**不需要**——线性扫描替代二分，三态比较常量无用（已移除）。　`// Go: tokens.go:comparison*`
- [x] 本地 `fn get_token_pos_of_node(file, node, include_jsdoc)->i32`（替代 scanner 的 DEFER 助手）　`// Go: scanner.go:GetTokenPosOfNode`
- [x] 本地 `fn get_scanner_for_source_file(text, lv, pos)->Scanner`（替代 scanner 的 DEFER 助手）　`// Go: scanner.go:GetScannerForSourceFile`
- [x] 本地谓词 `is_jsdoc_kind`/`is_jsdoc_node`/`is_non_whitespace_token`/`is_whitespace_only_jsx_text`/`is_jsx_child`/`is_property_name_literal`/`is_private_identifier`/`find_last_visible_node`　`// Go: ast/utilities.go:*`

### `lib.rs` — 公开入口（Go: `tokens.go`）

- [x] `pub fn get_touching_property_name(file, pos)->NodeId`（`allow_in_leading_trivia=false` + 过滤 `PrecedingTokenFilter::PropertyName`）　`// Go: tokens.go:GetTouchingPropertyName`
- [x] `pub fn get_touching_token(file, pos)->NodeId`（false，无过滤）　`// Go: tokens.go:GetTouchingToken`
- [x] `pub fn get_token_at_position(file, pos)->NodeId`（true，无过滤）　`// Go: tokens.go:GetTokenAtPosition`

### `lib.rs` — 核心 `getTokenAtPosition`（Go: `tokens.go`）

- [x] `fn get_token_at_position_core(file, pos, allow_in_leading_trivia, include_preceding: PrecedingTokenFilter)->NodeId`　`// Go: tokens.go:getTokenAtPosition`
  - [x] `test_node`（独立 fn）：包含判定 + `prev_subtree` 设置（end==position 且 filter 非 None 且非 reparsed）
  - [x] visitNode 逻辑（内联循环）：推进 `left`/`node_after_left`/`next`（跳 reparsed；JSDoc 不推 left）
  - [x] visitNodeList 特判：**拍平**为线性 visitNode 流；list-end 的 `left` 推进 / `prevSubtree=末元素` 两个 list 级特判作偏离（仅影响罕见 end-position 边界）
  - [x] 主循环：`visit_each_child_and_jsdoc` → prev_subtree 检查（`find_preceding_token_ex`）→ 无 next 时 scanner 扫 `[left,end)` 合成 token（`get_or_create_token`）→ 下钻 `current=next`
  - [x] scanner 路径 `token==Identifier || !is_token_kind`：JSDoc 返回 current，否则 `panic!`（保留断言）

### `lib.rs` — preceding / rightmost-valid（Go: `tokens.go`）

- [x] `pub fn find_preceding_token(file, pos)->Option<NodeId>`（=`find_preceding_token_ex(file,pos,None,false)`）　`// Go: tokens.go:FindPrecedingToken`
- [x] `pub fn find_preceding_token_ex(file, pos, start_node: Option<NodeId>, exclude_jsdoc)->Option<NodeId>`（递归 `fpt_find`：**线性**定位 foundChild、look-in-previous-child、JSDoc 分支（惰性）、childless/trailing 处理；结尾断言非纯空白 JsxText）　`// Go: tokens.go:FindPrecedingTokenEx`
- [x] `fn find_rightmost_valid_token(file, end_pos, containing, position, exclude_jsdoc)->Option<NodeId>`（递归 `frvt_find`，三情形：rightmostValidNode 递归 / 未访问尾部 token 扫描 / childless 节点；含 `should_visit_node`/`rightmost_visited_nodes`）　`// Go: tokens.go:findRightmostValidToken`

### `lib.rs` — next / child-of-kind（Go: `tokens.go`）

- [x] `pub fn find_next_token(file, previous_token, parent)->Option<NodeId>`（递归 `fnt_find`：token 起点==prev.End() 返回 / 含 prev 的节点下钻 / scanner 扫下一 token，`token_full_start==prev.end()` 校验，否则 `panic!`）　`// Go: tokens.go:FindNextToken`
- [x] `pub fn find_child_of_kind(file, containing, kind)->Option<NodeId>`（遍历子节点间隙 + 尾部用 scanner 找指定 Kind；直接 `scan.token()`，非 navigation token）　`// Go: tokens.go:FindChildOfKind`

### Cargo / crate 接线

- [x] `internal/astnav/Cargo.toml`（`name = "tsgo_astnav"` + path deps：ast/scanner/core；**dev-dep `tsgo_parser`** 用于测试 fixture）
- [x] 根 `Cargo.toml` workspace members 已含 `internal/astnav`
- [x] `lib.rs` 公开 5 个入口（`get_token_at_position`/`get_touching_token`/`get_touching_property_name`/`find_preceding_token(_ex)`/`find_next_token`/`find_child_of_kind`）+ `visit_each_child_and_jsdoc`/`get_start_of_node` + `pub struct SourceFile`

## TDD 推进顺序（tracer bullet → 增量）

1. **`VisitEachChildAndJSDoc` + `getNodeVisitor`**：先打通遍历器（依赖 P2 NodeVisitor），用简单源验证子节点访问顺序。
2. **`get_token_at_position` 最小路径**：纯 AST 命中（光标落在标识符上），不触发 scanner。配 tests.md 的 `pointer equality`（同位置返回同 NodeId）+ `JSDoc type assertion`（不 panic）。
3. **scanner 合成路径**：光标落在两 token 间的标点上 → 走 scanner 合成 + `GetOrCreateToken`。
4. **`find_preceding_token` + `find_rightmost_valid_token`**：实现 `TestUnitFindPrecedingToken` 两例（"after dot in jsdoc"、"after comma in parameter list"）。
5. **`find_next_token` / `find_child_of_kind`**：补齐。
6. baseline 对拍（GetTokenAtPosition/GetTouchingPropertyName/FindPrecedingToken/FindNextToken）→ 推迟 P10（需 TS submodule + Node.js）。

## 与 Go 的已知偏离（divergence）

- **`*ast.Node` → `NodeId`**：所有游标/返回值用索引；`nil`→`None`。合成 token 同样是 arena `NodeId`（经本地 token 缓存）。
- **`*ast.SourceFile` → 本地 `pub struct SourceFile`**：ast 的 SourceFile 节点不持有源文本，也无 token 缓存。astnav 自带导航上下文 `SourceFile{arena,root,text,language_variant,eof_token,token_cache}`，由 `tsgo_parser::ParseResult` 构造。函数签名为 `fn(file: &mut SourceFile, ...)`（合成/缓存 token 需 `&mut`）。
- **token 缓存就地实现（解阻塞）**：`GetOrCreateToken` 的指针相等由 astnav 自建缓存 `(parent,pos,end)->NodeId` 保证（不再依赖 ast SourceFile 缓存）。`TokenFlags` 在最小 `NodeData::Token` 上无存放位，故合成 token 不携带 TokenFlags（不影响 Kind/Pos/End/缓存键，故对所有断言不可见）。
- **scanner 就地驱动（解阻塞）**：`get_scanner_for_source_file`/`get_token_pos_of_node` 在 astnav 内本地实现（不依赖 scanner 的 DEFER 助手，不改 scanner）。`get_scanner_for_source_file` 每次查询 clone 源文本进 Scanner（`// PERF(port)`；Go 按引用共享）。
- **NodeVisitor hook 拍平 → `for_each_child` 线性流**：ast 无 hook 驱动遍历，`visit_each_child_and_jsdoc` 用 `for_each_child` 拍平出单一有序子节点流；调用方以**线性扫描**替代 Go 对 NodeList 的**二分查找**（`core::BinarySearchUniqueFunc`）——对排好序的子节点**结果等价**。受影响的 list 级特判（均为罕见 end-position 边界，baseline/P10 兜底）：
  - `getTokenAtPosition`：list 整体在 position 之前时 Go 用 `nodeList.End()` 推进 `left`（含尾随逗号），拍平用末元素 `.end()`；返回的 token 不变，仅合成 token 在 list 边界处的前导 trivia `pos` 可能不同。
  - `getTokenAtPosition`：`nodeList.End()==position` 时 Go 设 `prevSubtree=末元素`；拍平由 `test_node` 在末元素上设置（当末元素 end 与 list end 不等时可能不触发）。
  - `findNextToken`：list 级判定用 `node.End() > prev.Pos()`，单节点用 `> prev.End()`；拍平统一用单节点判定。
- **递归闭包 → 内部 `fn`**：`fpt_find`/`frvt_find`/`fnt_find` 取代 Go 的 `var find func(...)` 自引用闭包（捕获改传参：`file`/`position`/`exclude_jsdoc`）。visitor 可变局部（`next`/`left`/`prev_subtree`/`found_child`/`prev_child` 等）改为函数内 `let mut` + 内联循环，保证与 Go 同序更新。
- **`include_preceding` 回调 → `enum PrecedingTokenFilter`**：Go 传 `func(*Node)bool`；为避免闭包借用已被 `&mut` 持有的 `SourceFile`，建模为小枚举（`None`/`PropertyName`）。
- **panic 保留**：合成路径与结尾断言的 `panic!` 1:1 保留（`FindNextToken` baseline 测试在 P10 用 `catch_unwind` 兜）。
- **`get_node_visitor`/`comparison*` 常量未落地**：前者随 hook 拍平并入遍历器；后者随二分→线性而无用。

## 转交 / 推迟（DEFER）

- **baseline 对拍测试**（`TestGetTokenAtPosition`/`TestGetTouchingPropertyName`/`TestFindPrecedingToken`/`TestFindNextToken` 的 baseline 与 go-json 子测试）：依赖 `repo.TypeScriptSubmodulePath()` + `jstest`(Node.js 跑真实 `typescript`) + `testutil/baseline` 框架。`// DEFER(phase-10)`。
- **`TestMain`（baseline.Track）**：测试设施，随 P10 baseline 框架移植。
