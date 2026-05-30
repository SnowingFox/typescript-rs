# ast: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：2 文件 / 6 顶层 `func Test`（+ 6 个 `func Benchmark`）/ `TestDeepCloneNodeSanityCheck` 约 270 个表驱动子用例 + `positionmap` 5 个测试的内联断言。

> Go: `internal/ast/deepclone_test.go`（1 个 Test，~270 子用例）、`internal/ast/positionmap_test.go`（5 个 Test + 6 个 Benchmark）。
> ⚠️ **关键约束**：`deepclone_test.go` 用 `parsetestutil.ParseTypeScript(input)` 先把源码解析成树，再 `DeepCloneNode` 克隆并逐节点比对。**它依赖 parser（P3）**。因此该测试的完整收口（全 ~270 case red→green）**推迟到 P3**（`— DEFER(phase-3): blocked-by tsgo_parser`）。P2 内用**手工建树**（直接调 `NodeArena.new_*`）覆盖代表性 case，先验证 `DeepCloneNode`/`VisitEachChild`/`for_each_child`/`clone` 链路。`positionmap` 测试零外部依赖，**P2 即可全绿**。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/ast/positionmap_test.go` | `internal/ast/positionmap.rs`（`#[cfg(test)] mod tests`） | 5 Test（+ 6 Benchmark→`benches/`，可选） |
| `internal/ast/deepclone_test.go` | `internal/ast/deepclone.rs`（`#[cfg(test)] mod tests`，**P2 手工建树子集 + P3 全量**） | 1 Test（~270 子用例） |

---

## `positionmap_test.go`（P2 全绿）

> 5 个 `func Test*`，断言内联（非表驱动；`TestPositionMapMultipleNonASCII` 内含一个小表）。expected 全抄 Go 字面量。

### `TestPositionMapASCII` → `position_map_ascii`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `position_map_ascii` | 纯 ASCII 时 `is_ascii_only()==true`，且 `[0,len]` 内 `utf8_to_utf16(i)==i` 且 `utf16_to_utf8(i)==i` | `"const x = 1;"`（len 12），i∈[0,12] → 恒等 | `positionmap_test.go:TestPositionMapASCII` | |

### `TestPositionMapTwoByte` → `position_map_two_byte`

> `"const café = 1;\nconst x = 2;"`；`é`(U+00E9)=2 字节 UTF-8 / 1 code unit UTF-16（delta +1）。

| Rust 子断言 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `not_ascii_only` | 含非 ASCII | → `is_ascii_only()==false` | `TestPositionMapTwoByte` | |
| `before_e_identity` | é 之前（byte<10）恒等 | `utf8_to_utf16(i)==i`，i∈[0,10) | `TestPositionMapTwoByte` | |
| `at_e` | é 起点（byte 9）仍恒等 | `utf8_to_utf16(9)==9` | `TestPositionMapTwoByte` | |
| `after_e` | é 之后 delta=1 | `utf8_to_utf16(11)==10` | `TestPositionMapTwoByte` | |
| `at_x_second_line` | 第二行 `x` | `utf8_to_utf16(lastIndex("x"))==lastIndex-1` | `TestPositionMapTwoByte` | |
| `reverse_at_x` | 反向 | `utf16_to_utf8(lastIndex-1)==lastIndex("x")` | `TestPositionMapTwoByte` | |

### `TestPositionMapFourByte` → `position_map_four_byte`

> `const a = "🎉";\nconst b = 2;`；🎉(U+1F389)=4 字节 UTF-8 / 2 code unit UTF-16（delta +2）。

| Rust 子断言 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `not_ascii_only` | 含非 ASCII | → `is_ascii_only()==false` | `TestPositionMapFourByte` | |
| `at_b_forward` | 第二行 `b`，delta=2 | `utf8_to_utf16(lastIndex("b"))==lastIndex-2` | `TestPositionMapFourByte` | |
| `at_b_reverse` | 反向 | `utf16_to_utf8(lastIndex-2)==lastIndex("b")` | `TestPositionMapFourByte` | |

### `TestPositionMapMultipleNonASCII` → `position_map_multiple_non_ascii`

> `"à🎉x"`：à(U+00E0,2B/1U)、🎉(4B/2U)、x。表驱动 4 子用例。

| Rust 子用例 | utf8 → utf16（双向） | Go 对照 | 完成 |
|---|---|---|---|
| `pos_0` | `0 ↔ 0` | `TestPositionMapMultipleNonASCII` 表[0] | |
| `start_of_emoji` | `2 ↔ 1` | 表[1] | |
| `x` | `6 ↔ 3` | 表[2] | |
| `end` | `7 ↔ 4` | 表[3] | |

### `TestPositionMapRoundtrip` → `position_map_roundtrip`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `position_map_roundtrip` | 每个合法 UTF-16 位置 `utf16→utf8→utf16` 还原 | `"let café = \"🎉\"; // naïve"`，i∈[0,utf16Len] → `utf8_to_utf16(utf16_to_utf8(i))==i` | `positionmap_test.go:TestPositionMapRoundtrip` | |

### Benchmarks（→ `benches/`，criterion，可选，非 gate）

| Rust bench | Go 对照 | 完成 |
|---|---|---|
| `bench_compute_position_map_ascii` | `BenchmarkComputePositionMap_ASCII` | — |
| `bench_compute_position_map_non_ascii` | `BenchmarkComputePositionMap_NonASCII` | — |
| `bench_utf8_to_utf16_ascii` | `BenchmarkUTF8ToUTF16_ASCII` | — |
| `bench_utf8_to_utf16_non_ascii` | `BenchmarkUTF8ToUTF16_NonASCII` | — |
| `bench_utf16_to_utf8_non_ascii` | `BenchmarkUTF16ToUTF8_NonASCII` | — |
| `bench_compute_position_map_checker_ts`（读 `_submodules/.../checker.ts`，缺文件 skip） | `BenchmarkComputePositionMap_CheckerTS` | — |

---

## `deepclone_test.go`（P2 子集手工建树；P3 全量）

> Go: `func TestDeepCloneNodeSanityCheck`（表驱动，约 270 子用例）。
> **统一断言**（对每个 case 都一样）：`file := parsetestutil.ParseTypeScript(input)` → `clone := factory.DeepCloneNode(file)` → 用 `VisitEachChild` 收集子节点做 BFS：每对 `(original, copy)` 满足 **(1) 指针不同 `original != copy`，(2) 子节点数相等 `len(originalChildren)==len(copyChildren)`**，递归到底。
> Rust 对应：解析（P3）→ `arena.deep_clone_node(file)` → BFS 比对每对 `(orig_id, copy_id)` 满足 **(1) `orig_id != copy_id`（克隆产生新 NodeId），(2) `for_each_child` 收集的子 id 数相等**。
> 下表按节点种类（title 前缀）逐簇列出，每簇给子用例数 + 代表/边界输入。全部共用上述断言。`完成` 列 P2 默认 `—`（依赖 parser，DEFER P3），P2 手工建树覆盖的簇标注 `★`。

### 表达式（Expressions）

| 簇（title 前缀） | 子用例数 | 代表/边界输入 | Go 对照 | 完成 |
|---|---|---|---|---|
| `StringLiteral` | 2 | `;"test"`、`;'test'` | `TestDeepCloneNodeSanityCheck/Clone StringLiteral#*` | — |
| `NumericLiteral`/`BigIntLiteral` | 2 | `0`、`0n` | …/Clone NumericLiteral, BigIntLiteral | — |
| `BooleanLiteral`/`NullLiteral` | 3 | `true`、`false`、`null` | …/Clone BooleanLiteral#*, NullLiteral | — |
| `NoSubstitutionTemplateLiteral` | 1 | `` `` `` | …/Clone NoSubstitutionTemplateLiteral | — |
| `RegularExpressionLiteral` | 2 | `/a/`、`/a/g` | …/Clone RegularExpressionLiteral#* | — |
| `ThisExpression`/`SuperExpression`/`ImportExpression` | 3 | `this`、`super()`、`import()` | …/Clone ThisExpression, SuperExpression, ImportExpression | — |
| `PropertyAccess` | 11 | `a.b`、`a.#b`、`a?.b`、`a?.b.c`、`1..b`、`1.0.b`、`0x1.b`、`0b1.b`、`0o1.b`、`10e1.b`、`10E1.b` | …/Clone PropertyAccess#1..11 | — |
| `ElementAccess` | 3 | `a[b]`、`a?.[b]`、`a?.[b].c` | …/Clone ElementAccess#1..3 | — |
| `CallExpression` | 12 | `a()`、`a<T>()`、`a(b)`、`a(b).c`、`a?.(b)`、`a?.<T>(b).c`、`a<T,U>()`、`a<T,>()` … | …/Clone CallExpression#1..12 | — |
| `NewExpression` | 12 | `new a`、`new a.b`、`new a()`、`new a<T>(b).c` … | …/Clone NewExpression#1..12 | — |
| `TaggedTemplateExpression` | 2 | `` tag`` ``、`` tag<T>`` `` | …/Clone TaggedTemplateExpression#1..2 | — |
| `TypeAssertionExpression` | 1 | `<T>a` | …/Clone TypeAssertionExpression#1 | — |
| `FunctionExpression` | 8 | `(function(){})`、`(function*f(){})`、`(async function*f(){})`、`(function<T>(){})`、`(function():T{})` … | …/Clone FunctionExpression#1..8 | — |
| `ArrowFunction` | 9 | `a=>{}`、`()=>{}`、`<T>(a)=>{}`、`async<T>()=>{}`、`():T=>{}`、`()=>a` … | …/Clone ArrowFunction#1..9 | — |
| `Delete/TypeOf/Void/Await Expression` | 4 | `delete a`、`typeof a`、`void a`、`await a` | …/Clone DeleteExpression … AwaitExpression | — |
| `PrefixUnaryExpression` | 14 | `+a`、`++a`、`+ +a`、`+ ++a`、`-a`、`--a`、`+-a`、`~a`、`!a` … | …/Clone PrefixUnaryExpression#1..14 | — |
| `PostfixUnaryExpression` | 2 | `a++`、`a--` | …/Clone PostfixUnaryExpression#1..2 | — |
| `BinaryExpression` | 5 | `a,b`、`a+b`、`a**b`、`a instanceof b`、`a in b` | …/Clone BinaryExpression#1..5 | — |
| `ConditionalExpression` | 1 | `a?b:c` | …/Clone ConditionalExpression | — |
| `TemplateExpression` | 2 | `` `a${b}c` ``、`` `a${b}c${d}e` `` | …/Clone TemplateExpression#1..2 | — |
| `YieldExpression` | 3 | `(function*(){ yield })`、`yield a`、`yield*a` | …/Clone YieldExpression#1..3 | — |
| `SpreadElement` | 1 | `[...a]` | …/Clone SpreadElement | — |
| `ClassExpression` | 13 | `(class {})`、`(class a<T>{})`、`(class extends b implements c,d {})`、`(@a class {})` … | …/Clone ClassExpression#1..13 | — |
| `OmittedExpression` | 1 | `[,]` | …/Clone OmittedExpression | — |
| `ExpressionWithTypeArguments` | 1 | `a<T>` | …/Clone ExpressionWithTypeArguments | — |
| `As/Satisfies/NonNull Expression` | 3 | `a as T`、`a satisfies T`、`a!` | …/Clone AsExpression, SatisfiesExpression, NonNullExpression | — |
| `MetaProperty` | 2 | `new.target`、`import.meta` | …/Clone MetaProperty#1..2 | — |
| `ArrayLiteralExpression` | 5 | `[]`、`[a]`、`[a,]`、`[,a]`、`[...a]` | …/Clone ArrayLiteralExpression#1..5 | — |
| `ObjectLiteralExpression` + 属性赋值 | 5 | `({})`、`({a,})`、`({a})`(Shorthand)、`({a:b})`(Property)、`({...a})`(Spread) | …/Clone ObjectLiteralExpression#*, ShorthandPropertyAssignment, PropertyAssignment, SpreadAssignment | — |

### 语句（Statements）★（P2 手工建树覆盖代表簇）

| 簇 | 子用例数 | 代表/边界输入 | Go 对照 | 完成 |
|---|---|---|---|---|
| `Block`/`EmptyStatement` | 2 | `{}`、`;` | …/Clone Block, EmptyStatement | — |
| `VariableStatement` | 5 | `var a`、`let a`、`const a=b`、`using a=b`、`await using a=b` | …/Clone VariableStatement#1..5 | — |
| `IfStatement` | 10 | `if(a);`、`if(a);else;`、`if(a);else if(b){}`、`if(a){}else{}` … | …/Clone IfStatement#1..10 | — |
| `DoStatement`/`WhileStatement` | 4 | `do;while(a);`、`do{}while(a);`、`while(a);`、`while(a){}` | …/Clone DoStatement#*, WhileStatement#* | — |
| `ForStatement` | 6 | `for(;;);`、`for(a;;);`、`for(var a;;);`、`for(;a;);`、`for(;;a);`、`for(;;){}` | …/Clone ForStatement#1..6 | — |
| `ForInStatement`/`ForOfStatement` | 9 | `for(a in b);`、`for(var a of b){}`、`for await(a of b);` … | …/Clone ForInStatement#*, ForOfStatement#* | — |
| `Continue/Break/Return Statement` | 6 | `continue`、`continue a`、`break`、`break a`、`return`、`return a` | …/Clone ContinueStatement#*, BreakStatement#*, ReturnStatement#* | — |
| `WithStatement` | 2 | `with(a);`、`with(a){}` | …/Clone WithStatement#1..2 | — |
| `SwitchStatement`+子句 | 5 | `switch(a){}`、`case b:`、`case b:;`、`default:`、`default:;` | …/Clone SwitchStatement, CaseClause#*, DefaultClause#* | — |
| `Labeled/Throw/Debugger Statement` | 3 | `a:;`、`throw a`、`debugger` | …/Clone LabeledStatement, ThrowStatement, DebuggerStatement | — |
| `TryStatement` | 3 | `try{}catch{}`、`try{}finally{}`、`try{}catch{}finally{}` | …/Clone TryStatement#1..3 | — |

### 声明（Declarations）

| 簇 | 子用例数 | 代表/边界输入 | Go 对照 | 完成 |
|---|---|---|---|---|
| `FunctionDeclaration` | 9 | `export default function(){}`、`function*f(){}`、`async function*f(){}`、`function f<T>(){}`、`function f():T{}`、`function f();` | …/Clone FunctionDeclaration#1..9 | — |
| `ClassDeclaration` | 15 | `class a{}`、`class a extends b implements c,d {}`、`export default class<T>{}`、`@a class b{}`、`@a export class b{}`、`export @a class b{}` | …/Clone ClassDeclaration#1..15 | — |
| `InterfaceDeclaration` | 4 | `interface a{}`、`interface a<T>{}`、`interface a extends b,c{}` | …/Clone InterfaceDeclaration#1..4 | — |
| `TypeAliasDeclaration` | 2 | `type a=b`、`type a<T>=b` | …/Clone TypeAliasDeclaration#1..2 | — |
| `EnumDeclaration` | 3 | `enum a{}`、`enum a{b}`、`enum a{b=c}` | …/Clone EnumDeclaration#1..3 | — |
| `ModuleDeclaration` | 8 | `module a{}`、`module a.b{}`、`module "a";`、`namespace a.b{}`、`global;`、`global{}` | …/Clone ModuleDeclaration#1..8 | — |
| `ImportEqualsDeclaration` | 8 | `import a=b`、`import a=require("b")`、`export import a=b`、`import type a=b.c` | …/Clone ImportEqualsDeclaration#1..8 | — |
| `ImportDeclaration` | 17 | `import "a"`、`import a from "b"`、`import type * as a from "b"`、`import {a as b} from "c"`、`import {"a" as b} from "c"`、`import a,* as b from "c"`、`import {} from "a" with {b:"c"}` | …/Clone ImportDeclaration#1..16（含重复编号 17 条） | — |
| `ExportAssignment`/`NamespaceExportDeclaration` | 3 | `export = a`、`export default a`、`export as namespace a` | …/Clone ExportAssignment#*, NamespaceExportDeclaration | — |
| `ExportDeclaration` | 38 | `export * from "a"`、`export type * as a from "b"`、`export {type a as "b"} from "c"`、`export {"a" as "b"}`、`export {} from "a" with {b:"c"}` … | …/Clone ExportDeclaration#1..38 | — |

### 类型（Types）

| 簇 | 子用例数 | 代表/边界输入 | Go 对照 | 完成 |
|---|---|---|---|---|
| `KeywordTypeNode` | 13 | `type T=any/unknown/never/void/undefined/null/object/string/symbol/number/bigint/boolean/intrinsic` | …/Clone KeywordTypeNode#1..13 | — |
| `TypePredicateNode` | 4 | `f():asserts a`、`f():asserts a is b`、`f():asserts this`、`f():asserts this is b` | …/Clone TypePredicateNode#1..4 | — |
| `TypeReferenceNode` | 4 | `type T=a`、`a.b`、`a<U>`、`a.b<U>` | …/Clone TypeReferenceNode#1..4 | — |
| `FunctionTypeNode`/`ConstructorTypeNode` | 7 | `()=>a`、`<T>()=>a`、`new ()=>a`、`abstract new ()=>a` | …/Clone FunctionTypeNode#*, ConstructorTypeNode#* | — |
| `TypeQueryNode` | 3 | `typeof a`、`typeof a.b`、`typeof a<U>` | …/Clone TypeQueryNode#1..3 | — |
| `TypeLiteralNode` | 2 | `type T={}`、`type T={a}` | …/Clone TypeLiteralNode#1..2 | — |
| `ArrayTypeNode`/`TupleTypeNode`/`Rest`/`Optional`/`NamedTupleMember` | 9 | `a[]`、`[]`、`[a]`、`[a,]`、`[...a]`、`[a?]`、`[a:b]`、`[a?:b]`、`[...a:b]` | …/Clone ArrayTypeNode, TupleTypeNode#*, RestTypeNode, OptionalTypeNode, NamedTupleMember#* | — |
| `UnionTypeNode`/`IntersectionTypeNode` | 6 | `a\|b`、`a\|b\|c`、`\|a\|b`、`a&b`、`a&b&c`、`&a&b` | …/Clone UnionTypeNode#*, IntersectionTypeNode#* | — |
| `ConditionalTypeNode`/`InferTypeNode` | 3 | `a extends b?c:d`、`a extends infer b?c:d`、`a extends infer b extends c?d:e` | …/Clone ConditionalTypeNode, InferTypeNode#* | — |
| `Parenthesized/This/TypeOperator/IndexedAccess` | 6 | `(U)`、`this`、`keyof U`、`readonly U[]`、`unique symbol`、`a[b]` | …/Clone ParenthesizedTypeNode, ThisTypeNode, TypeOperatorNode#*, IndexedAccessTypeNode | — |
| `MappedTypeNode` | 9 | `{[a in b]:c}`、`{[a in b as c]:d}`、`{+readonly [a in b]:c}`、`{[a in b]+?:c}`、`{[a in b]:c;d}` | …/Clone MappedTypeNode#1..9 | — |
| `LiteralTypeNode` | 10 | `null`、`true`、`false`、`""`、`''`、`` `` ``、`0`、`0n`、`-0`、`-0n` | …/Clone LiteralTypeNode#1..10 | — |
| `TemplateTypeNode` | 2 | `` `a${b}c` ``、`` `a${b}c${d}e` `` | …/Clone TemplateTypeNode#1..2 | — |
| `ImportTypeNode` | 8 | `import(a)`、`import(a).b<U>`、`typeof import(a).b`、`import(a,{with:{b:"c"}})` | …/Clone ImportTypeNode#1..7（含重复编号 8 条） | — |

### 成员 / 签名 / 绑定（Members / Signatures / Binding）

| 簇 | 子用例数 | 代表/边界输入 | Go 对照 | 完成 |
|---|---|---|---|---|
| `PropertySignature` | 9 | `interface I{a}`、`{readonly a}`、`{"a"}`、`{0n}`、`{[a]}`、`{a?}`、`{a:b}` | …/Clone PropertySignature#1..9 | — |
| `MethodSignature` | 10 | `{a()}`、`{0n()}`、`{[a]()}`、`{a?()}`、`{a<T>()}`、`{a(b):c}` | …/Clone MethodSignature#1..10 | — |
| `CallSignature`/`ConstructSignature` | 8 | `{()}`、`{():a}`、`{(p)}`、`{<T>()}`、`{new ()}`、`{new <T>()}` | …/Clone CallSignature#*, ConstructSignature#* | — |
| `IndexSignatureDeclaration` | 3 | `{[a]}`、`{[a:b]}`、`{[a:b]:c}` | …/Clone IndexSignatureDeclaration#1..3 | — |
| `PropertyDeclaration` | 15 | `class C{a}`、`{readonly a}`、`{static a}`、`{accessor a}`、`{#a}`、`{a!}`、`{a=b}`、`{@a b}` | …/Clone PropertyDeclaration#1..15 | — |
| `MethodDeclaration` | 15 | `{a()}`、`{#a()}`、`{a<T>()}`、`{a(){}}`、`{@a b(){}}`、`{static a(){}}`、`{async a(){}}` | …/Clone MethodDeclaration#1..15 | — |
| `GetAccessorDeclaration` | 12 | `{get a()}`、`{get #a()}`、`{get a():b}`、`{@a get b(){}}`、`{static get a(){}}` | …/Clone GetAccessorDeclaration#1..12 | — |
| `SetAccessorDeclaration` | 12 | `{set a()}`、`{set #a()}`、`{set a(b):c}`、`{@a set b(){}}`、`{static set a(){}}` | …/Clone SetAccessorDeclaration#1..12 | — |
| `ConstructorDeclaration` | 6 | `{constructor()}`、`{constructor(b):c}`、`{@a constructor(){}}`、`{private constructor(){}}` | …/Clone ConstructorDeclaration#1..6 | — |
| `ClassStaticBlock`/`SemicolonClassElement` | 2 | `class C{static {}}`、`class C{;}` | …/Clone ClassStaticBlockDeclaration, SemicolonClassElement#1 | — |
| `ParameterDeclaration` | 7 | `f(a)`、`f(a:b)`、`f(a=b)`、`f(a?)`、`f(...a)`、`f(this)`、`f(a,)` | …/Clone ParameterDeclaration#1..7 | — |
| `ObjectBindingPattern` | 12 | `f({})`、`f({a})`、`f({a=b})`、`f({a:b=c})`、`f({0:b})`、`f({[a]:b})`、`f({...a})`、`f({a:{}})`、`f({a:[]})` | …/Clone ObjectBindingPattern#1..12 | — |
| `ArrayBindingPattern` | 9 | `f([])`、`f([,])`、`f([a])`、`f([a,,b])`、`f([a=b])`、`f([...a])`、`f([{}])`、`f([[]])` | …/Clone ArrayBindingPattern#1..9 | — |
| `TypeParameterDeclaration` | 7 | `f<T>()`、`f<in T>()`、`f<T extends U>()`、`f<T=U>()`、`f<T extends U=V>()`、`f<T,U>()`、`f<T,>()` | …/Clone TypeParameterDeclaration#1..7 | — |

### JSX

| 簇 | 子用例数 | 代表/边界输入 | Go 对照 | 完成 |
|---|---|---|---|---|
| `JsxElement` | 11 | `<a></a>`、`<this></this>`、`<a:b></a:b>`、`<a.b></a.b>`、`<a<b>></a>`、`<a b></a>`、`<a>{b}</a>`、`<a><b/></a>`、`<a><></></a>` | …/Clone JsxElement1..11 | — |
| `JsxSelfClosingElement` | 6 | `<a />`、`<this />`、`<a:b />`、`<a.b />`、`<a<b> />`、`<a b/>` | …/Clone JsxSelfClosingElement1..6 | — |
| `JsxFragment` | 6 | `<></>`、`<>b</>`、`<>{b}</>`、`<><b></b></>`、`<><b /></>`、`<><></></>` | …/Clone JsxFragment1..6 | — |
| `JsxAttribute`/`JsxSpreadAttribute` | 9 | `<a b/>`、`<a b:c/>`、`<a b="c"/>`、`<a b={c}/>`、`<a b=<c></c>/>`、`<a b=<></>/>`、`<a {...b}/>` | …/Clone JsxAttribute1..8, JsxSpreadAttribute | — |

> 全部簇合计约 270 子用例，覆盖几乎每种 `Kind`。P3 parser 落地后，整张表用 `rstest` 参数化（每行一个 `case`，input 直接抄 Go），统一跑上述"指针不同 + 子节点数相等"断言。

## P2 内的手工建树补充测试（不依赖 parser）

> 为在 P2 验证 `DeepCloneNode`/`VisitEachChild`/`for_each_child`/`clone` 链路，用 `NodeArena.new_*` 手工建小树，断言克隆语义。expected 取自 deepclone.go 逻辑。

| Rust 测试 | 验证内容 | 构造 → 断言 | 依据 | 完成 |
|---|---|---|---|---|
| `deep_clone_identifier_is_new_node` | 叶子被强制克隆 | `new_identifier("a")` → clone id ≠ 原 id，`text` 相等 | deepclone.go:getDeepCloneVisitor | ✓ |
| `deep_clone_property_access_children` | 子节点全为新 + 数量一致 | 手建 `a.b`（PropertyAccess）→ BFS：每对 id 不同、子数相等 | deepclone.go | ✓ |
| `deep_clone_call_with_args` | 列表克隆 | 手建 `a(b,c)` → arguments 列表节点全新、长度一致 | deepclone.go（VisitNodes 克隆分支） | ✓ |
| `deep_clone_synthetic_location` | 合成位置 | `deep_clone_node` 后所有节点 `loc==(-1,-1)`；尾逗号列表末元素 `(-2,-2)` | deepclone.go（syntheticLocation 分支） | ✓ |
| `deep_clone_reparse_sets_parent_and_flag` | reparse 变体 | `deep_clone_reparse` 后 `set_parent_in_children` 生效（子 `parent` 指向新父）、根置 `NodeFlagsReparsed`、`loc` 非合成 | deepclone.go:DeepCloneReparse | ✓ |
| `for_each_child_count_matches_visit_each_child` | 两遍历器一致 | 同一手建树，`for_each_child` 计数 == `get_children`（经 `visit_each_child` 收集）计数 | deepclone_test.go:getChildren | ✓ |

> **P3 回填（持续扩展，parser-backed）**：`TestDeepCloneNodeSanityCheck` 的真实解析版现位于 `internal/parser/deepclone_test.rs::deep_clone_node_sanity_check`（`ast` crate 不能依赖 `tsgo_parser`，否则倒置依赖边）。它用 `parse_source_file` 解析片段，再 `arena.deep_clone_node(file)` 跑 Go 的 BFS 不变量（每对 `(orig, clone)` id 不同 + 子节点数相等）。case 数随 parser 切片扩展：Round 1 = 28、Round 2 = ~60、Round 3 = ~95、**Round 4 = ~107**（新增 type-args-in-call/instantiation/负字面量类型/装饰器/import 属性/export 属性/import-type 属性，以及独立的 **TSX** 表：JSX 元素·fragment·命名空间成员，和 **JSON** 表：对象·数组）。剩余缺口为 JSDoc 簇（随 JSDoc+reparser 子系统整体 DEFER），逼近 Go 的 ~270 全表。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（positionmap 5 个逐断言；deepclone 1 个按 ~270 子用例分簇全列）
- [x] 每个表驱动子用例都已覆盖（positionmap `MultipleNonASCII` 4 行逐列；deepclone 按 title 前缀分簇 + 子用例数 + 代表/边界输入，无遗漏簇）
- [x] expected 值均取自 Go 测试字面量（positionmap 偏移数值、deepclone 输入串、统一断言语义）
- [x] 每条带 `// Go:` 锚点（`<file>_test.go:<TestFunc>[/<case>]`）
- [x] 与 impl.md 双向对齐：`PositionMap`/`DeepCloneNode`/`NodeVisitor`/`for_each_child`/`new_*`/`set_parent_in_children` 在 impl.md 均有实现 TODO 承载
- [x] benchmark 标注为可选、非 gate

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `TestDeepCloneNodeSanityCheck` 全 ~270 case red→green | 依赖 `parsetestutil.ParseTypeScript`（parser）。**P3 已回填代表性子集**（28 case，`internal/parser/deepclone_test.rs`）；剩余 case 随声明/类型/JSX 产生式落地而扩展 | P3（已部分完成）→ 随 parser 切片扩展 |
| benchmark `*_CheckerTS`（读 TS 子模块） | 子模块依赖 + 非 gate | P10 / 可选 |
| 各 `Kind` 经真实解析→克隆→打印的端到端一致 | 需 parser+printer | P10 parity |
