# printer: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：3 文件 / **105 `func Test`** / 约 430 子用例（`TestEmit` 一个函数就含 ~290 表驱动子用例）。

这是本 phase 工作量最大的 tests.md。105 个 `func Test` 全部逐一列出；表驱动函数（`TestEmit`/`TestEscape*`/`TestIsRecognizedTripleSlashComment`/`TestParenthesizeBinary`/`TestParenthesizeBinaryExpressionMixingNullishCoalescing`/`TestTypeEraser`-引用）逐子用例列出，expected 取自 Go 字面量。

> 字符串约定：`\n` = 换行，`\t` = 制表符，`\"`/`\'` = 转义引号，反引号原样。input/output 均为 Go 测试里的字面量。

## 实现进度（本轮 subagent）

| Go 测试文件 | 顶层函数 | 已移植并全绿 | 推迟 |
|---|---|---|---|
| `utilities_test.go` | 4 | **4 ✓**（`escape_string_table`/`escape_non_ascii_string_table`/`escape_jsx_attribute_string_table`/`is_recognized_triple_slash_comment_table`，全子用例对齐 Go 字面量） | 0 |
| `namegenerator_test.go` | 36 | **31 ✓**（temp 1-3/scoped/scoped_reserved、loop 1-3/scoped、unique 1-2/scoped、unique_private 1-2/scoped 共 16；**[6p]** node-based 15：identifier 1-3、node_cached、export_assignment、class_expression、function_declaration 1-2、class_declaration 1-2、method 1-2、private_name_for_method、computed_property_name、other） | **5**（namespace 1-4 `— DEFER(phase-4)` blocked-by binder `Locals`/`isUniqueLocalName`；import/export `— DEFER` blocked-by `GetExternalModuleName`）|
| `printer_test.go` | 65 | **~46**（`TestEmit` 全部已实现 NodeKind 家族子用例 + 21 parenthesizer func） | 见下（解析器受限子用例 + 37 parenthesizer + 4 name-gen-emit transform） |

### Round 2：emit 核心 + parenthesizer 进度

- **`TestEmit`（~290 子用例）**：表达式/语句/声明/类型/JSX 全家族已逐 NodeKind 移植并全绿（Rust 测试按家族分布在 `printer_test.rs`/`emit_expressions_test.rs`/`emit_statements_test.rs`/`emit_declarations_test.rs`/`emit_types_test.rs`/`emit_jsx_test.rs`，每条 expected 取自 Go 字面量）。**解析器受限推迟的子用例**（`tsgo_parser` 限制，非 printer 责任）：`ImportExpression`(`import()` 死循环，`#[ignore]`)、`VariableStatement` 的 `using`/`await using`、`ModuleDeclaration` 的 `global{}`、`TupleType` 的 `[a?]`/`[a?: b]`。
- **parenthesizer（58 func）**：已移植 `TestParenthesizeBinary`(15 子用例) + `Conditional1/2` + `SpreadElement1` + `Call4` + `New2` + `AsExpression`（共 ~21），见 `parenthesizer_test.rs`（合成 AST 经 `check_synthetic`）。其余 ~37 条合成 AST 用例 round 3 续。
- **name-gen + transform-emit（4 func）**：`TestNameGeneration`/`No/TrailingCommaAfterTransform`/`PartiallyEmittedExpression` 仍 DEFER（依赖 transformers/visitor + node-based 名字）。
- 测试总数：`cargo test -p tsgo_printer` = **160 unit + 18 doctests 全绿**（1 `#[ignore]`：`import()`）。gate-code.sh **C1–C8 GREEN**。

额外（每函数单测，§8.6）：`emitflags`/`generatedidentifierflags`/`emittextwriter`/`textwriter`/`emitcontext`/`factory` 各文件均带兄弟 `<stem>_test.rs`（Go 无对应测试，按行为级单测补齐）。

下面表格中 `完成` 列：本轮已落地的用例标 `✓`，emit/parenthesizer/node-based 名字相关标 `—`（DEFER phase-4）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/printer/utilities_test.go` | `internal/printer/utilities.rs`（`#[cfg(test)] mod tests`） | 4 |
| `internal/printer/namegenerator_test.go` | `internal/printer/namegenerator.rs`（或 `tests/namegenerator.rs`，external crate 测试） | 36 |
| `internal/printer/printer_test.go` | `internal/printer/tests/printer.rs`（external `printer_test` crate 风格） | 65 |
| 合计 | | **105** |

公共测试设施（执行期需先于本包/在 P10 testutil 提供）：`parsetestutil.ParseTypeScript` / `CheckDiagnostics` / `MarkSyntheticRecursive`、`emittestutil.CheckEmit`。Rust 侧建等价 helper（解析 → emit → 断言整串）。

---

## `utilities_test.go`（4 func）

### `TestEscapeString`（表驱动，`EscapeString(s, quoteChar)`）

| Rust 测试 | input(s, quote) → expected | Go 对照 | 完成 |
|---|---|---|---|
| `escape_string[0]` | `("", Double)` → `` （空） | `TestEscapeString/[0]` | |
| `escape_string[1]` | `("abc", Double)` → `abc` | `.../[1]` | |
| `escape_string[2]` | `("ab\"c", Double)` → `ab\"c` | `.../[2]` | |
| `escape_string[3]` | `("ab\tc", Double)` → `ab\tc` | `.../[3]` | |
| `escape_string[4]` | `("ab\nc", Double)` → `ab\nc` | `.../[4]` | |
| `escape_string[5]` | `("ab'c", Double)` → `ab'c` | `.../[5]` | |
| `escape_string[6]` | `("ab'c", Single)` → `ab\'c` | `.../[6]` | |
| `escape_string[7]` | `("ab\"c", Single)` → `ab"c` | `.../[7]` | |
| `escape_string[8]` | `("ab`c", Backtick)` → `` ab\`c `` | `.../[8]` | |
| `escape_string[9]` | `("\u001f", Backtick)` → `\u001F` | `.../[9]` | |

### `TestEscapeNonAsciiString`（表驱动，`escapeNonAsciiString`）

| Rust 测试 | input(s, quote) → expected | Go 对照 | 完成 |
|---|---|---|---|
| `escape_non_ascii[0]` | `("", Double)` → 空 | `TestEscapeNonAsciiString/[0]` | |
| `escape_non_ascii[1]` | `("abc", Double)` → `abc` | `.../[1]` | |
| `escape_non_ascii[2]` | `("ab\"c", Double)` → `ab\"c` | `.../[2]` | |
| `escape_non_ascii[3]` | `("ab\tc", Double)` → `ab\tc` | `.../[3]` | |
| `escape_non_ascii[4]` | `("ab\nc", Double)` → `ab\nc` | `.../[4]` | |
| `escape_non_ascii[5]` | `("ab'c", Double)` → `ab'c` | `.../[5]` | |
| `escape_non_ascii[6]` | `("ab'c", Single)` → `ab\'c` | `.../[6]` | |
| `escape_non_ascii[7]` | `("ab\"c", Single)` → `ab"c` | `.../[7]` | |
| `escape_non_ascii[8]` | `("ab`c", Backtick)` → `` ab\`c `` | `.../[8]` | |
| `escape_non_ascii[9]` | `("ab\u008fc", Double)` → `ab\u008Fc` | `.../[9]` | |
| `escape_non_ascii[10]` | `("𝟘𝟙", Double)` → `\uD835\uDFD8\uD835\uDFD9`（代理对） | `.../[10]` | |

### `TestEscapeJsxAttributeString`（表驱动，`escapeJsxAttributeString`）

| Rust 测试 | input(s, quote) → expected | Go 对照 | 完成 |
|---|---|---|---|
| `escape_jsx_attr[0]` | `("", Double)` → 空 | `TestEscapeJsxAttributeString/[0]` | |
| `escape_jsx_attr[1]` | `("abc", Double)` → `abc` | `.../[1]` | |
| `escape_jsx_attr[2]` | `("ab\"c", Double)` → `ab&quot;c` | `.../[2]` | |
| `escape_jsx_attr[3]` | `("ab\tc", Double)` → `ab&#x9;c` | `.../[3]` | |
| `escape_jsx_attr[4]` | `("ab\nc", Double)` → `ab&#xA;c` | `.../[4]` | |
| `escape_jsx_attr[5]` | `("ab'c", Double)` → `ab'c` | `.../[5]` | |
| `escape_jsx_attr[6]` | `("ab'c", Single)` → `ab&apos;c` | `.../[6]` | |
| `escape_jsx_attr[7]` | `("ab\"c", Single)` → `ab"c` | `.../[7]` | |
| `escape_jsx_attr[8]` | `("ab\u008fc", Double)` → `ab\u008Fc`（保留非转义高位） | `.../[8]` | |
| `escape_jsx_attr[9]` | `("𝟘𝟙", Double)` → `𝟘𝟙`（原样） | `.../[9]` | |

### `TestIsRecognizedTripleSlashComment`（表驱动，~40 子用例）

逐子用例（`s` → expected bool；未指定 commentRange 时按 SingleLine 处理）：

- `("", MultiLine)` → false；`("", SingleLine)` → false
- `("/a")` → false；`("//")` → false；`("//a")` → false；`("///")` → false；`("///a")` → false
- `("///<reference path=\"foo\" />")` → true；`types` → true；`lib` → true；`no-default-lib` → true
- `("///<amd-dependency path=\"foo\" />")` → true；`("///<amd-module />")` → true
- 带前导空格 `"/// <reference path=\"foo\" />"`（path/types/lib/no-default-lib）→ true；`"/// <amd-dependency path=\"foo\" />"` → true；`"/// <amd-module />"` → true
- 无空格自闭合 `"/// <reference path=\"foo\"/>"`（path/types/lib/no-default-lib）→ true；`"/// <amd-dependency path=\"foo\"/>"` → true；`"/// <amd-module/>"` → true
- 单引号 `"/// <reference path='foo' />"`（path/types/lib/no-default-lib）→ true；`"/// <amd-dependency path='foo' />"` → true
- 带尾随空格 `"/// <reference path=\"foo\" />  "`（path/types/lib/no-default-lib）→ true；`amd-dependency` → true；`amd-module` → true
- `"/// <foo />"` → false；`"/// <reference />"` → false；`"/// <amd-dependency />"` → false

| Rust 测试 | 验证内容 | Go 对照 | 完成 |
|---|---|---|---|
| `is_recognized_triple_slash_comment_table`（rstest 参数化覆盖上述全部 ~40 行） | reference/amd-dependency/amd-module 各种引号/空格/自闭合识别；非法返回 false | `utilities_test.go:TestIsRecognizedTripleSlashComment` | |

---

## `namegenerator_test.go`（36 func，逐 func 单场景）

`ec=NewEmitContext(); g=NameGenerator{Context:ec}`（带节点的用例额外 `GetTextOfNode`）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `temp_variable_1` | 连续 temp 递增 | 2×`NewTempVariable` → `"_a"`,`"_b"` | `TestTempVariable1` | |
| `temp_variable_2` | prefix/suffix 包裹 | `Ex{Prefix:"A",Suffix:"B"}`×2 → `"A_aB"`,`"A_bB"` | `TestTempVariable2` | |
| `temp_variable_3` | 同节点缓存稳定 | 同 name 两次 → `"_a"`,`"_a"` | `TestTempVariable3` | |
| `temp_variable_scoped` | push scope 后重置计数 | scope 内第二个 → `"_a"`,`"_a"` | `TestTempVariableScoped` | |
| `temp_variable_scoped_reserved` | ReservedInNestedScopes 占名 | reserved `_a` → 嵌套 `"_b"` | `TestTempVariableScopedReserved` | |
| `loop_variable_1` | 首循环变量 `_i` | 2×`NewLoopVariable` → `"_i"`,`"_a"` | `TestLoopVariable1` | |
| `loop_variable_2` | prefix/suffix | `Ex{A,B}`×2 → `"A_iB"`,`"A_aB"` | `TestLoopVariable2` | |
| `loop_variable_3` | 同节点缓存 | 同 name 两次 → `"_i"`,`"_i"` | `TestLoopVariable3` | |
| `loop_variable_scoped` | scope 内 `_i` 复用 | scope 内 → `"_i"`,`"_i"` | `TestLoopVariableScoped` | |
| `unique_name_1` | 唯一名递增 | 2×`NewUniqueName("foo")` → `"foo_1"`,`"foo_2"` | `TestUniqueName1` | |
| `unique_name_2` | 同节点缓存（object identity） | 同 name 两次 → `"foo_1"`,`"foo_1"` | `TestUniqueName2` | |
| `unique_name_scoped` | scope 内仍递增（匹配 Strada，已知不正确） | scope 内第二个 → `"foo_2"` | `TestUniqueNameScoped` | |
| `unique_private_name_1` | 私有唯一名递增 | 2×`NewUniquePrivateName("#foo")` → `"#foo_1"`,`"#foo_2"` | `TestUniquePrivateName1` | |
| `unique_private_name_2` | 同节点缓存 | 同 name 两次 → `"#foo_1"`,`"#foo_1"` | `TestUniquePrivateName2` | |
| `unique_private_name_scoped` | 私有名嵌套作用域恒保留 | scope 内 → `"#foo_2"` | `TestUniquePrivateNameScoped` | |
| `generated_name_for_identifier_1` | 基于标识符节点 | 合成 `Identifier("f")` → `"f_1"` | `TestGeneratedNameForIdentifier1` | ✓ **[6p]** |
| `generated_name_for_identifier_2` | prefix/suffix 直接拼（optimistic） | `Ex{a,b}` → `"afb"` | `TestGeneratedNameForIdentifier2` | ✓ **[6p]** |
| `generated_name_for_identifier_3` | 嵌套生成名 + `_1` | 对 `afb` 再生成 → `"afb_1"` | `TestGeneratedNameForIdentifier3` | ✓ **[6p]** |
| `generated_name_for_namespace_1` | 不冲突则复用名 | `namespace foo {}` → `"foo"` | `TestGeneratedNameForNamespace1` | — DEFER(phase-4) blocked-by binder `Locals`/`isUniqueLocalName` |
| `generated_name_for_namespace_2` | 与局部冲突 → 生成 | `namespace foo { var foo; }` → `"foo_1"` | `TestGeneratedNameForNamespace2` | — DEFER(phase-4) blocked-by binder `Locals` |
| `generated_name_for_namespace_3` | 未作用域避免碰撞 | 两个 `foo` 命名空间 → `"foo_1"`,`"foo_2"` | `TestGeneratedNameForNamespace3` | — DEFER(phase-4) blocked-by binder `Locals` |
| `generated_name_for_namespace_4` | 作用域内（匹配 Strada，已知不正确） | scope 各自 → `"foo_1"`,`"foo_2"` | `TestGeneratedNameForNamespace4` | — DEFER(phase-4) blocked-by binder `Locals` |
| `generated_name_for_node_cached` | 同节点两次同名（node-id 缓存） | 合成 `Identifier("foo")` 两次生成名 → `"foo_1"`,`"foo_1"`（divergence：Go 用 namespace，binder-free 端口用 identifier，缓存语义同） | `TestGeneratedNameForNodeCached` | ✓ **[6p]** |
| `generated_name_for_import` | import * as foo | `import * as foo from 'foo'` → `"foo_1"` | `TestGeneratedNameForImport` | — DEFER(phase-4) blocked-by `GetExternalModuleName` |
| `generated_name_for_export` | export * as foo | `export * as foo from 'foo'` → `"foo_1"` | `TestGeneratedNameForExport` | — DEFER(phase-4) blocked-by `GetExternalModuleName` |
| `generated_name_for_function_declaration_1` | function（有名）→ 递归 name | 合成 `function f(){}` → `"f_1"` | `TestGeneratedNameForFunctionDeclaration1` | ✓ **[6p]** |
| `generated_name_for_function_declaration_2` | export default function（无名）| 合成无名 function → `"default_1"` | `TestGeneratedNameForFunctionDeclaration2` | ✓ **[6p]** |
| `generated_name_for_class_declaration_1` | class（有名）→ 递归 name | 合成 `class C {}` → `"C_1"` | `TestGeneratedNameForClassDeclaration1` | ✓ **[6p]** |
| `generated_name_for_class_declaration_2` | export default class（无名）| 合成无名 class → `"default_1"` | `TestGeneratedNameForClassDeclaration2` | ✓ **[6p]** |
| `generated_name_for_export_assignment` | export default expr | 合成 `export default 0` → `"default_1"` | `TestGeneratedNameForExportAssignment` | ✓ **[6p]** |
| `generated_name_for_class_expression` | class 表达式 | 合成 `ClassExpression{}` → `"class_1"` | `TestGeneratedNameForClassExpression` | ✓ **[6p]** |
| `generated_name_for_method_1` | 方法名（identifier）| 合成 method `m` → `"m_1"` | `TestGeneratedNameForMethod1` | ✓ **[6p]** |
| `generated_name_for_method_2` | 数字字面量名 → temp | 合成 method `0` → `"_a"` | `TestGeneratedNameForMethod2` | ✓ **[6p]** |
| `generated_private_name_for_method` | 私有生成名 | 合成 method `m` 私有生成名 → `"#m_1"` | `TestGeneratedPrivateNameForMethod` | ✓ **[6p]** |
| `generated_name_for_computed_property_name` | 计算属性名 → temp（reserved-in-nested-scopes）| 合成 `ComputedPropertyName([x])` → `"_a"` | `TestGeneratedNameForComputedPropertyName` | ✓ **[6p]** |
| `generated_name_for_other` | 任意节点 → temp | 合成 object literal → `"_a"` | `TestGeneratedNameForOther` | ✓ **[6p]** |

---

## `printer_test.go`（65 func）

### `TestEmit`（1 func，~290 表驱动子用例 —— 本 phase 的核心，逐 NodeKind 全列）

驱动：`parsetestutil.ParseTypeScript(input, jsx)` → `CheckDiagnostics` → `emittestutil.CheckEmit(nil, file, output)`。`jsx:true` 的用例在末尾 JSX 段标注。下面**每条 = 一个 `t.Run(title)` 子用例**，逐条对齐。

#### 字面量 / 基础表达式
- `StringLiteral#1`: `;"test"` → `;\n"test";`
- `StringLiteral#2`: `;'test'` → `;\n'test';`
- `NumericLiteral#1`: `0` → `0;`
- `NumericLiteral#2`: `10_000` → `10000;`
- `BigIntLiteral#1`: `0n` → `0n;`
- `BigIntLiteral#2`: `10_000n` → `10000n;`（TODO：迁移后保留分隔符）
- `BooleanLiteral#1`: `true` → `true;`
- `BooleanLiteral#2`: `false` → `false;`
- `NoSubstitutionTemplateLiteral`: `` `` `` → `` ``; ``
- `NoSubstitutionTemplateLiteral#2`: `` `\n` `` → `` `\n`; ``
- `RegularExpressionLiteral#1`: `/a/` → `/a/;`
- `RegularExpressionLiteral#2`: `/a/g` → `/a/g;`
- `NullLiteral`: `null` → `null;`
- `ThisExpression`: `this` → `this;`
- `SuperExpression`: `super()` → `super();`
- `ImportExpression`: `import()` → `import();`

#### PropertyAccess（#1–#14）
- `#1` `a.b`→`a.b;`；`#2` `a.#b`→`a.#b;`；`#3` `a?.b`→`a?.b;`；`#4` `a?.b.c`→`a?.b.c;`
- `#5` `1..b`→`1..b;`；`#6` `1.0.b`→`1.0.b;`；`#7` `0x1.b`→`0x1.b;`；`#8` `0b1.b`→`0b1.b;`；`#9` `0o1.b`→`0o1.b;`
- `#10` `10e1.b`→`10e1.b;`；`#11` `10E1.b`→`10E1.b;`；`#12` `a.b?.c`→`a.b?.c;`
- `#13` `a\n.b`→`a\n    .b;`；`#14` `a.\nb`→`a.\n    b;`

#### ElementAccess（#1–#3）
- `#1` `a[b]`→`a[b];`；`#2` `a?.[b]`→`a?.[b];`；`#3` `a?.[b].c`→`a?.[b].c;`

#### CallExpression（#1–#13，注 #12 被注释跳过）
- `#1` `a()`→`a();`；`#2` `a<T>()`→`a<T>();`；`#3` `a(b)`→`a(b);`；`#4` `a<T>(b)`→`a<T>(b);`
- `#5` `a(b).c`→`a(b).c;`；`#6` `a<T>(b).c`→`a<T>(b).c;`；`#7` `a?.(b)`→`a?.(b);`；`#8` `a?.<T>(b)`→`a?.<T>(b);`
- `#9` `a?.(b).c`→`a?.(b).c;`；`#10` `a?.<T>(b).c`→`a?.<T>(b).c;`；`#11` `a<T, U>()`→`a<T, U>();`；`#13` `a?.b()`→`a?.b();`

#### NewExpression（#1–#12）
- `#1` `new a`→`new a;`；`#2` `new a.b`→`new a.b;`；`#3` `new a()`→`new a();`；`#4` `new a.b()`→`new a.b();`
- `#5` `new a<T>()`→`new a<T>();`；`#6` `new a.b<T>()`→`new a.b<T>();`；`#7` `new a(b)`→`new a(b);`；`#8` `new a.b(c)`→`new a.b(c);`
- `#9` `new a<T>(b)`→`new a<T>(b);`；`#10` `new a.b<T>(c)`→`new a.b<T>(c);`；`#11` `new a(b).c`→`new a(b).c;`；`#12` `new a<T>(b).c`→`new a<T>(b).c;`

#### Tagged template / 类型断言 / 函数 / 箭头
- `TaggedTemplateExpression#1`: `` tag`` `` → `` tag ``; ``；`#2`: `` tag<T>`` `` → `` tag<T> ``; ``
- `TypeAssertionExpression#1`: `<T>a` → `<T>a;`
- `FunctionExpression#1..8`: `(function(){})`→`(function () { });`；`(function f(){})`→`(function f() { });`；`(function*f(){})`→`(function* f() { });`；`(async function f(){})`→`(async function f() { });`；`(async function*f(){})`→`(async function* f() { });`；`(function<T>(){})`→`(function <T>() { });`；`(function(a){})`→`(function (a) { });`；`(function():T{})`→`(function (): T { });`
- `ArrowFunction#1..9`: `a=>{}`→`a => { };`；`()=>{}`→`() => { };`；`(a)=>{}`→`(a) => { };`；`<T>(a)=>{}`→`<T>(a) => { };`；`async a=>{}`→`async (a) => { };`；`async()=>{}`→`async () => { };`；`async<T>()=>{}`→`async <T>() => { };`；`():T=>{}`→`(): T => { };`；`()=>a`→`() => a;`

#### 一元 / 二元 / 条件 / 模板 / yield / spread
- `DeleteExpression` `delete a`→`delete a;`；`TypeOfExpression` `typeof a`→`typeof a;`；`VoidExpression` `void a`→`void a;`；`AwaitExpression` `await a`→`await a;`
- `PrefixUnaryExpression#1..14`: `+a`→`+a;`；`++a`→`++a;`；`+ +a`→`+ +a;`；`+ ++a`→`+ ++a;`；`-a`→`-a;`；`--a`→`--a;`；`- -a`→`- -a;`；`- --a`→`- --a;`；`+-a`→`+-a;`；`+--a`→`+--a;`；`-+a`→`-+a;`；`-++a`→`-++a;`；`~a`→`~a;`；`!a`→`!a;`
- `PostfixUnaryExpression#1..2`: `a++`→`a++;`；`a--`→`a--;`
- `BinaryExpression#1..7`: `a,b`→`a, b;`；`a+b`→`a + b;`；`a**b`→`a ** b;`；`a instanceof b`→`a instanceof b;`；`a in b`→`a in b;`；`a\n&& b`→`a\n    && b;`；`a &&\nb`→`a &&\n    b;`
- `ConditionalExpression#1..5`: `a?b:c`→`a ? b : c;`；`a\n?b:c`→`a\n    ? b : c;`；`a?\nb:c`→`a ?\n    b : c;`；`a?b\n:c`→`a ? b\n    : c;`；`a?b:\nc`→`a ? b :\n    c;`
- `TemplateExpression#1..2`: `` `a${b}c` ``→`` `a${b}c`; ``；`` `a${b}c${d}e` ``→`` `a${b}c${d}e`; ``
- `YieldExpression#1..3`: `(function*() { yield })`→`(function* () { yield; });`；`yield a`变体→`(function* () { yield a; });`；`yield*a`变体→`(function* () { yield* a; });`
- `SpreadElement`: `[...a]`→`[...a];`

#### Class 表达式 / 杂项表达式
- `ClassExpression#1..13`: `(class {})`→`(class {\n});`；`(class a {})`→`(class a {\n});`；`(class<T>{})`→`(class<T> {\n});`；`(class a<T>{})`→`(class a<T> {\n});`；`(class extends b {})`→`(class extends b {\n});`；`(class a extends b {})`→`(class a extends b {\n});`；`(class implements b {})`→`(class implements b {\n});`；`(class a implements b {})`→`(class a implements b {\n});`；`(class implements b, c {})`→`(class implements b, c {\n});`；`(class a implements b, c {})`→`(class a implements b, c {\n});`；`(class extends b implements c, d {})`→`(class extends b implements c, d {\n});`；`(class a extends b implements c, d {})`→`(class a extends b implements c, d {\n});`；`(@a class {})`→`(\n@a\nclass {\n});`
- `OmittedExpression`: `[,]`→`[,];`
- `ExpressionWithTypeArguments`: `a<T>`→`a<T>;`
- `AsExpression`: `a as T`→`a as T;`；`SatisfiesExpression`: `a satisfies T`→`a satisfies T;`；`NonNullExpression`: `a!`→`a!;`
- `MetaProperty#1..2`: `new.target`→`new.target;`；`import.meta`→`import.meta;`

#### 数组/对象字面量 + 属性赋值
- `ArrayLiteralExpression#1..6`: `[]`→`[];`；`[a]`→`[a];`；`[a,]`→`[a,];`；`[,a]`→`[, a];`；`[...a]`→`[...a];`；`const array = [/* comment */];`→`const array = [ /* comment */];`
- `ObjectLiteralExpression#1..2`: `({})`→`({});`；`({a,})`→`({ a, });`
- `ShorthandPropertyAssignment`: `({a})`→`({ a });`；`PropertyAssignment`: `({a:b})`→`({ a: b });`；`SpreadAssignment`: `({...a})`→`({ ...a });`

#### 语句
- `Block`: `{}`→`{ }`
- `VariableStatement#1..5`: `var a`→`var a;`；`let a`→`let a;`；`const a = b`→`const a = b;`；`using a = b`→`using a = b;`；`await using a = b`→`await using a = b;`
- `EmptyStatement`: `;`→`;`
- `IfStatement#1..10`: `if(a);`→`if (a)\n    ;`；`if(a);else;`→`if (a)\n    ;\nelse\n    ;`；`if(a);else{}`→`if (a)\n    ;\nelse { }`；`if(a);else if(b);`→`if (a)\n    ;\nelse if (b)\n    ;`；`if(a);else if(b) {}`→`if (a)\n    ;\nelse if (b) { }`；`if(a) {}`→`if (a) { }`；`if(a) {} else;`→`if (a) { }\nelse\n    ;`；`if(a) {} else {}`→`if (a) { }\nelse { }`；`if(a) {} else if(b);`→`if (a) { }\nelse if (b)\n    ;`；`if(a) {} else if(b){}`→`if (a) { }\nelse if (b) { }`
- `DoStatement#1..2`: `do;while(a);`→`do\n    ;\nwhile (a);`；`do {} while(a);`→`do { } while (a);`
- `WhileStatement#1..2`: `while(a);`→`while (a)\n    ;`；`while(a) {}`→`while (a) { }`
- `ForStatement#1..6`: `for(;;);`→`for (;;)\n    ;`；`for(a;;);`→`for (a;;)\n    ;`；`for(var a;;);`→`for (var a;;)\n    ;`；`for(;a;);`→`for (; a;)\n    ;`；`for(;;a);`→`for (;; a)\n    ;`；`for(;;){}`→`for (;;) { }`
- `ForInStatement#1..3`: `for(a in b);`→`for (a in b)\n    ;`；`for(var a in b);`→`for (var a in b)\n    ;`；`for(a in b){}`→`for (a in b) { }`
- `ForOfStatement#1..6`: `for(a of b);`→`for (a of b)\n    ;`；`for(var a of b);`→`for (var a of b)\n    ;`；`for(a of b){}`→`for (a of b) { }`；`for await(a of b);`→`for await (a of b)\n    ;`；`for await(var a of b);`→`for await (var a of b)\n    ;`；`for await(a of b){}`→`for await (a of b) { }`
- `ContinueStatement#1..2`: `continue`→`continue;`；`continue a`→`continue a;`
- `BreakStatement#1..2`: `break`→`break;`；`break a`→`break a;`
- `ReturnStatement#1..2`: `return`→`return;`；`return a`→`return a;`
- `WithStatement#1..2`: `with(a);`→`with (a)\n    ;`；`with(a){}`→`with (a) { }`
- `SwitchStatement`: `switch (a) {}`→`switch (a) {\n}`
- `CaseClause#1..2`: `switch (a) {case b:}`→`switch (a) {\n    case b:\n}`；`switch (a) {case b:;}`→`switch (a) {\n    case b: ;\n}`
- `DefaultClause#1..2`: `switch (a) {default:}`→`switch (a) {\n    default:\n}`；`switch (a) {default:;}`→`switch (a) {\n    default: ;\n}`
- `LabeledStatement`: `a:;`→`a: ;`；`ThrowStatement`: `throw a`→`throw a;`
- `TryStatement#1..3`: `try {} catch {}`→`try { }\ncatch { }`；`try {} finally {}`→`try { }\nfinally { }`；`try {} catch {} finally {}`→`try { }\ncatch { }\nfinally { }`
- `DebuggerStatement`: `debugger`→`debugger;`

#### 声明
- `FunctionDeclaration#1..9`: `export default function(){}`→`export default function () { }`；`function f(){}`→`function f() { }`；`function*f(){}`→`function* f() { }`；`async function f(){}`→`async function f() { }`；`async function*f(){}`→`async function* f() { }`；`function f<T>(){}`→`function f<T>() { }`；`function f(a){}`→`function f(a) { }`；`function f():T{}`→`function f(): T { }`；`function f();`→`function f();`
- `ClassDeclaration#1..15`: `class a {}`→`class a {\n}`；`class a<T>{}`→`class a<T> {\n}`；`class a extends b {}`→`class a extends b {\n}`；`class a implements b {}`→`class a implements b {\n}`；`class a implements b, c {}`→`class a implements b, c {\n}`；`class a extends b implements c, d {}`→`class a extends b implements c, d {\n}`；`export default class {}`→`export default class {\n}`；`export default class<T>{}`→`export default class<T> {\n}`；`export default class extends b {}`→`export default class extends b {\n}`；`export default class implements b {}`→`export default class implements b {\n}`；`export default class implements b, c {}`→`export default class implements b, c {\n}`；`export default class extends b implements c, d {}`→`export default class extends b implements c, d {\n}`；`@a class b {}`→`@a\nclass b {\n}`；`@a export class b {}`→`@a\nexport class b {\n}`；`export @a class b {}`→`export \n@a\nclass b {\n}`
- `InterfaceDeclaration#1..4`: `interface a {}`→`interface a {\n}`；`interface a<T>{}`→`interface a<T> {\n}`；`interface a extends b {}`→`interface a extends b {\n}`；`interface a extends b, c {}`→`interface a extends b, c {\n}`
- `TypeAliasDeclaration#1..2`: `type a = b`→`type a = b;`；`type a<T> = b`→`type a<T> = b;`
- `EnumDeclaration#1..3`: `enum a{}`→`enum a {\n}`；`enum a{b}`→`enum a {\n    b\n}`；`enum a{b=c}`→`enum a {\n    b = c\n}`
- `ModuleDeclaration#1..8`: `module a{}`→`module a { }`；`module a.b{}`→`module a.b { }`；`module "a";`→`module "a";`；`module "a"{}`→`module "a" { }`；`namespace a{}`→`namespace a { }`；`namespace a.b{}`→`namespace a.b { }`；`global;`→`global;`；`global{}`→`global { }`
- `ImportEqualsDeclaration#1..8`: `import a = b`→`import a = b;`；`import a = b.c`→`import a = b.c;`；`import a = require("b")`→`import a = require("b");`；`export import a = b`→`export import a = b;`；`export import a = require("b")`→`export import a = require("b");`；`import type a = b`→`import type a = b;`；`import type a = b.c`→`import type a = b.c;`；`import type a = require("b")`→`import type a = require("b");`

#### ImportDeclaration（标题含重复编号，逐条按出现顺序）
- `import "a"`→`import "a";`；`import a from "b"`→`import a from "b";`；`import type a from "b"`→`import type a from "b";`；`import * as a from "b"`→`import * as a from "b";`；`import type * as a from "b"`→`import type * as a from "b";`；`import {} from "b"`→`import {} from "b";`；`import type {} from "b"`→`import type {} from "b";`；`import { a } from "b"`→`import { a } from "b";`；`import type { a } from "b"`→`import type { a } from "b";`；`import { a as b } from "c"`→`import { a as b } from "c";`；`import type { a as b } from "c"`→`import type { a as b } from "c";`；`import { "a" as b } from "c"`→`import { "a" as b } from "c";`；`import type { "a" as b } from "c"`→`import type { "a" as b } from "c";`；`import a, {} from "b"`→`import a, {} from "b";`；`import a, * as b from "c"`→`import a, * as b from "c";`；`import {} from "a" with {}`→`import {} from "a" with {};`；`import {} from "a" with { b: "c" }`→`import {} from "a" with { b: "c" };`；`import {} from "a" with { "b": "c" }`→`import {} from "a" with { "b": "c" };`

#### Export
- `ExportAssignment#1..2`: `export = a`→`export = a;`；`export default a`→`export default a;`
- `NamespaceExportDeclaration`: `export as namespace a`→`export as namespace a;`
- `ExportDeclaration#1..38`（逐条）：`export * from "a"`→`export * from "a";`；`export type * from "a"`→`export type * from "a";`；`export * as a from "b"`→`export * as a from "b";`；`export type * as a from "b"`→`export type * as a from "b";`；`export { } from "a"`→`export {} from "a";`；`export type { } from "a"`→`export type {} from "a";`；`export { a } from "b"`→`export { a } from "b";`；`export { type a } from "b"`→`export { type a } from "b";`；`export type { a } from "b"`→`export type { a } from "b";`；`export { a as b } from "c"`→`export { a as b } from "c";`；`export { type a as b } from "c"`→`export { type a as b } from "c";`；`export type { a as b } from "c"`→`export type { a as b } from "c";`；`export { a as "b" } from "c"`→`export { a as "b" } from "c";`；`export { type a as "b" } from "c"`→`export { type a as "b" } from "c";`；`export type { a as "b" } from "c"`→`export type { a as "b" } from "c";`；`export { "a" } from "b"`→`export { "a" } from "b";`；`export { type "a" } from "b"`→`export { type "a" } from "b";`；`export type { "a" } from "b"`→`export type { "a" } from "b";`；`export { "a" as b } from "c"`→`export { "a" as b } from "c";`；`export { type "a" as b } from "c"`→`export { type "a" as b } from "c";`；`export type { "a" as b } from "c"`→`export type { "a" as b } from "c";`；`export { "a" as "b" } from "c"`→`export { "a" as "b" } from "c";`；`export { type "a" as "b" } from "c"`→`export { type "a" as "b" } from "c";`；`export type { "a" as "b" } from "c"`→`export type { "a" as "b" } from "c";`；`export { }`→`export {};`；`export type { }`→`export type {};`；`export { a }`→`export { a };`；`export { type a }`→`export { type a };`；`export type { a }`→`export type { a };`；`export { a as b }`→`export { a as b };`；`export { type a as b }`→`export { type a as b };`；`export type { a as b }`→`export type { a as b };`；`export { a as "b" }`→`export { a as "b" };`；`export { type a as "b" }`→`export { type a as "b" };`；`export type { a as "b" }`→`export type { a as "b" };`；`export {} from "a" with {}`→`export {} from "a" with {};`；`export {} from "a" with { b: "c" }`→`export {} from "a" with { b: "c" };`；`export {} from "a" with { "b": "c" }`→`export {} from "a" with { "b": "c" };`

#### 类型节点
- `KeywordTypeNode#1..13`: `type T = any/unknown/never/void/undefined/null/object/string/symbol/number/bigint/boolean/intrinsic` → 各自 `type T = <kw>;`
- `TypePredicateNode#1..4`: `function f(): asserts a`→`...asserts a;`；`asserts a is b`；`asserts this`；`asserts this is b`（均 `;` 结尾，无 body）
- `TypeReferenceNode#1..4`: `type T = a`→`type T = a;`；`a.b`；`a<U>`；`a.b<U>`
- `FunctionTypeNode#1..3`: `() => a`；`<T>() => a`；`(a) => b`（`type T = ...;`）
- `ConstructorTypeNode#1..4`: `new () => a`；`new <T>() => a`；`new (a) => b`；`abstract new () => a`
- `TypeQueryNode#1..3`: `typeof a`；`typeof a.b`；`typeof a<U>`
- `TypeLiteralNode#1..2`: `type T = {}`→`type T = {};`；`type T = {a}`→`type T = {\n    a;\n};`
- `ArrayTypeNode`: `type T = a[]`→`type T = a[];`
- `TupleTypeNode#1..3`: `[]`→`type T = [\n];`；`[a]`→`type T = [\n    a\n];`；`[a,]`→`type T = [\n    a\n];`
- `RestTypeNode`: `[...a]`→`type T = [\n    ...a\n];`；`OptionalTypeNode`: `[a?]`→`type T = [\n    a?\n];`
- `NamedTupleMember#1..3`: `[a: b]`→`type T = [\n    a: b\n];`；`[a?: b]`→`...a?: b...`；`[...a: b]`→`...…a: b...`
- `UnionTypeNode#1..3`: `a | b`→`type T = a | b;`；`a | b | c`；`| a | b`→`type T = a | b;`
- `IntersectionTypeNode#1..3`: `a & b`；`a & b & c`；`& a & b`→`type T = a & b;`
- `ConditionalTypeNode`: `a extends b ? c : d`→`type T = a extends b ? c : d;`
- `InferTypeNode#1..2`: `a extends infer b ? c : d`；`a extends infer b extends c ? d : e`
- `ParenthesizedTypeNode`: `(U)`→`type T = (U);`；`ThisTypeNode`: `this`→`type T = this;`
- `TypeOperatorNode#1..3`: `keyof U`；`readonly U[]`；`unique symbol`
- `IndexedAccessTypeNode`: `a[b]`→`type T = a[b];`
- `MappedTypeNode#1..9`: `{ [a in b]: c }`→`type T = {\n    [a in b]: c;\n};`；`{ [a in b as c]: d }`；`{ readonly [a in b]: c }`；`{ +readonly ... }`；`{ -readonly ... }`；`{ [a in b]?: c }`；`{ [a in b]+?: c }`；`{ [a in b]-?: c }`；`{ [a in b]: c; d }`→`...[a in b]: c;\n    d;...`
- `LiteralTypeNode#1..10`: `null`；`true`；`false`；`""`；`''`；`` `` ``；`0`；`0n`；`-0`；`-0n`（各 `type T = <lit>;`）
- `TemplateTypeNode#1..2`: `` `a${b}c` ``；`` `a${b}c${d}e` ``（`type T = ...;`）
- `ImportTypeNode`（标题含重复 #6/#7）: `import(a)`；`import(a).b`；`import(a).b<U>`；`typeof import(a)`；`typeof import(a).b`；`import(a, { with: { } })`→`type T = import(a, { with: {} });`；`import(a, { with: { b: "c" } })`；`import(a, { with: { "b": "c" } })`

#### 签名成员
- `PropertySignature#1..9`: `interface I {a}`→`interface I {\n    a;\n}`；`{readonly a}`；`{"a"}`；`{'a'}`；`{0}`；`{0n}`；`{[a]}`；`{a?}`；`{a: b}`
- `MethodSignature#1..10`: `{a()}`；`{"a"()}`；`{'a'()}`；`{0()}`；`{0n()}`；`{[a]()}`；`{a?()}`；`{a<T>()}`；`{a(): b}`；`{a(b): c}`
- `CallSignature#1..4`: `{()}`；`{():a}`；`{(p)}`；`{<T>()}`
- `ConstructSignature#1..4`: `{new ()}`；`{new ():a}`；`{new (p)}`；`{new <T>()}`
- `IndexSignatureDeclaration#1..3`: `{[a]}`；`{[a: b]}`；`{[a: b]: c}`

#### 类成员
- `PropertyDeclaration#1..15`: `class C {a}`→`class C {\n    a;\n}`；`{readonly a}`；`{static a}`；`{accessor a}`；`{"a"}`；`{'a'}`；`{0}`；`{0n}`；`{[a]}`；`{#a}`；`{a?}`；`{a!}`；`{a: b}`；`{a = b}`；`{@a b}`→`class C {\n    @a\n    b;\n}`
- `MethodDeclaration#1..15`: `{a()}`；`{"a"()}`；`{'a'()}`；`{0()}`；`{0n()}`；`{[a]()}`；`{#a()}`；`{a?()}`；`{a<T>()}`；`{a(): b}`；`{a(b): c}`；`{a() {} }`→`...a() { }...`；`{@a b() {} }`；`{static a() {} }`；`{async a() {} }`
- `GetAccessorDeclaration#1..12`: `{get a()}`..`{get a(): b}`/`{get a(b): c}`/`{get a() {} }`/`{@a get b() {} }`/`{static get a() {} }`
- `SetAccessorDeclaration#1..12`: 同 get 的 set 对应（`set a()`..`static set a() {}`）
- `ConstructorDeclaration#1..6`: `{constructor()}`；`{constructor(): b}`；`{constructor(b): c}`；`{constructor() {} }`；`{@a constructor() {} }`→`...constructor() { }...`（装饰器被丢）；`{private constructor() {} }`
- `ClassStaticBlockDeclaration`: `class C {static { }}`→`class C {\n    static { }\n}`
- `SemicolonClassElement#1`: `class C {;}`→`class C {\n    ;\n}`

#### 参数 / 绑定模式 / 类型参数
- `ParameterDeclaration#1..6`: `function f(a)`→`function f(a);`；`(a: b)`；`(a = b)`；`(a?)`；`(...a)`；`(this)`
- `ObjectBindingPattern#1..12`: `function f({})`→`function f({});`；`{a}`；`{a = b}`；`{a: b}`；`{a: b = c}`；`{"a": b}`；`{'a': b}`；`{0: b}`；`{[a]: b}`；`{...a}`；`{a: {}}`；`{a: []}`
- `ArrayBindingPattern#1..9`: `function f([])`→`function f([]);`；`[,]`；`[a]`；`[a, b]`；`[a, , b]`；`[a = b]`；`[...a]`；`[{}]`；`[[]]`
- `TypeParameterDeclaration#1..6`: `function f<T>();`；`<in T>`；`<T extends U>`；`<T = U>`；`<T extends U = V>`；`<T, U>`

#### JSX（jsx:true）
- `JsxElement1..12`: `<a></a>`→`<a></a>;`；`<this></this>`；`<a:b></a:b>`；`<a.b></a.b>`；`<a<b>></a>`→`<a<b>></a>;`；`<a b></a>`；`<a>b</a>`；`<a>{b}</a>`；`<a><b></b></a>`；`<a><b /></a>`；`<a><></></a>`；`JsxElement12`（多行注释保留，见源：`<a>\n    {/* missing */}\n    {\n        // foo\n    }\n</a>`→`<a>\n    {/* missing */}\n    {\n    // foo\n    }\n</a>;`）
- `JsxSelfClosingElement1..6`: `<a />`→`<a />;`；`<this />`；`<a:b />`；`<a.b />`；`<a<b> />`；`<a b/>`→`<a b/>;`
- `JsxFragment1..6`: `<></>`→`<></>;`；`<>b</>`；`<>{b}</>`；`<><b></b></>`；`<><b /></>`；`<><></></>`
- `JsxAttribute1..8`: `<a b/>`→`<a b/>;`；`<a b:c/>`；`<a b="c"/>`；`<a b='c'/>`；`<a b={c}/>`；`<a b=<c></c>/>`；`<a b=<c />/>`；`<a b=<></>/>`
- `JsxSpreadAttribute`: `<a {...b}/>`→`<a {...b}/>;`

| 收口检查 | Go 对照 | 完成 |
|---|---|---|
| 以上每个 `t.Run(title)` 子用例都有对应 Rust `#[test]`/`rstest` 行（~290 条） | `printer_test.go:TestEmit` | |

### 表达式 parenthesizer（单场景，每 func 一个合成 AST + 一个断言）

| Rust 测试 | 验证内容 | expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parenthesize_decorator` | 装饰器表达式加括号 | `@(a + b)\nclass C {\n}` | `TestParenthesizeDecorator` | |
| `parenthesize_computed_property_name` | 计算属性名逗号表达式加括号 | `class C {\n    [(a, b)];\n}` | `TestParenthesizeComputedPropertyName` | |
| `parenthesize_array_literal` | 数组元素逗号表达式 | `[(a, b)];` | `TestParenthesizeArrayLiteral` | |
| `parenthesize_property_access_1` | `(a, b).c` | `(a, b).c;` | `TestParenthesizePropertyAccess1` | |
| `parenthesize_property_access_2` | 可选链外加括号 | `(a?.b).c;` | `TestParenthesizePropertyAccess2` | |
| `parenthesize_property_access_3` | new 无参加括号 | `(new a).b;` | `TestParenthesizePropertyAccess3` | |
| `parenthesize_element_access_1` | `(a, b)[c]` | `(a, b)[c];` | `TestParenthesizeElementAccess1` | |
| `parenthesize_element_access_2` | `(a?.b)[c]` | `(a?.b)[c];` | `TestParenthesizeElementAccess2` | |
| `parenthesize_element_access_3` | `(new a)[b]` | `(new a)[b];` | `TestParenthesizeElementAccess3` | |
| `parenthesize_call_1` | `(a, b)()` | `(a, b)();` | `TestParenthesizeCall1` | |
| `parenthesize_call_2` | `(a?.b)()` | `(a?.b)();` | `TestParenthesizeCall2` | |
| `parenthesize_call_3` | `(new C)()` | `(new C)();` | `TestParenthesizeCall3` | |
| `parenthesize_call_4` | 实参逗号表达式 | `a((b, c));` | `TestParenthesizeCall4` | |
| `parenthesize_new_1` | `new (a, b)()` | `new (a, b)();` | `TestParenthesizeNew1` | |
| `parenthesize_new_2` | new 包裹 call | `new (C());` | `TestParenthesizeNew2` | |
| `parenthesize_new_3` | new 实参逗号 | `new C((a, b));` | `TestParenthesizeNew3` | |
| `parenthesize_tagged_template_1` | `(a, b) ` tag | `` (a, b) ``; `` | `TestParenthesizeTaggedTemplate1` | |
| `parenthesize_tagged_template_2` | `(a?.b) ` tag | `` (a?.b) ``; `` | `TestParenthesizeTaggedTemplate2` | |
| `parenthesize_type_assertion_1` | `<T>(a + b)` | `<T>(a + b);` | `TestParenthesizeTypeAssertion1` | |
| `parenthesize_arrow_function_1` | 箭头体对象加括号 | `() => ({});` | `TestParenthesizeArrowFunction1` | |
| `parenthesize_arrow_function_2` | 箭头体对象成员访问 | `() => ({}.a);` | `TestParenthesizeArrowFunction2` | |
| `parenthesize_delete` | `delete (a + b)` | `delete (a + b);` | `TestParenthesizeDelete` | |
| `parenthesize_void` | `void (a + b)` | `void (a + b);` | `TestParenthesizeVoid` | |
| `parenthesize_type_of` | `typeof (a + b)` | `typeof (a + b);` | `TestParenthesizeTypeOf` | |
| `parenthesize_await` | `await (a + b)` | `await (a + b);` | `TestParenthesizeAwait` | |

### `TestParenthesizeBinary`（表驱动，15 子用例，`output+";"`）

| Rust 测试 | (left, op, right) → output | Go 对照 | 完成 |
|---|---|---|---|
| `bin[0]` | `(_, ,)` 逗号 → `l, r` | `TestParenthesizeBinary/l, r` | |
| `bin[1]` | 左为 `+`，op `,` → `ll + lr, r` | `.../ll + lr, r` | |
| `bin[2]` | op `*`，左 `+` → `(ll + lr) * r` | `.../(ll + lr) * r` | |
| `bin[3]` | op `*`，右 `+` → `l * (rl + rr)` | `.../l * (rl + rr)` | |
| `bin[4]` | op `+`，左 `*` → `ll * lr + r` | `.../ll * lr + r` | |
| `bin[5]` | op `+`，右 `*` → `l + rl * rr` | `.../l + rl * rr` | |
| `bin[6]` | op `/`，左 `*` → `ll * lr / r` | `.../ll * lr / r` | |
| `bin[7]` | op `/`，左 `**` → `ll ** lr / r` | `.../ll ** lr / r` | |
| `bin[8]` | op `**`，左 `*` → `(ll * lr) ** r` | `.../(ll * lr) ** r` | |
| `bin[9]` | op `**`，左 `**` → `(ll ** lr) ** r` | `.../(ll ** lr) ** r` | |
| `bin[10]` | op `*`，右 `*` → `l * rl * rr` | `.../l * rl * rr` | |
| `bin[11]` | op `|`，右 `|` → `l | rl | rr` | `.../l \| rl \| rr` | |
| `bin[12]` | op `&`，右 `&` → `l & rl & rr` | `.../l & rl & rr` | |
| `bin[13]` | op `^`，右 `^` → `l ^ rl ^ rr` | `.../l ^ rl ^ rr` | |
| `bin[14]` | op `&&`，右箭头 → `l && (() => { })` | `.../l && (() => { })` | |

### 条件 / yield / spread / 杂项 parenthesizer（单场景）

| Rust 测试 | 验证内容 | expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parenthesize_conditional_1` | 条件 condition 逗号 | `(a, b) ? c : d;` | `TestParenthesizeConditional1` | |
| `parenthesize_conditional_2` | condition 赋值 | `(a = b) ? c : d;` | `TestParenthesizeConditional2` | |
| `parenthesize_conditional_3` | condition 箭头 | `(() => { }) ? a : b;` | `TestParenthesizeConditional3` | |
| `parenthesize_conditional_4` | condition yield | `(yield) ? a : b;` | `TestParenthesizeConditional4` | |
| `parenthesize_conditional_5` | whenTrue 逗号 | `a ? (b, c) : d;` | `TestParenthesizeConditional5` | |
| `parenthesize_conditional_6` | whenFalse 逗号 | `a ? b : (c, d);` | `TestParenthesizeConditional6` | |
| `parenthesize_yield_1` | yield 逗号操作数 | `yield (a, b);` | `TestParenthesizeYield1` | |
| `parenthesize_spread_element_1` | 数组 spread 逗号 | `[...(a, b)];` | `TestParenthesizeSpreadElement1` | |
| `parenthesize_spread_element_2` | call 实参 spread 逗号 | `a(...(b, c));` | `TestParenthesizeSpreadElement2` | |
| `parenthesize_spread_element_3` | new 实参 spread 逗号 | `new a(...(b, c));` | `TestParenthesizeSpreadElement3` | |
| `parenthesize_expression_with_type_arguments` | `(a, b)<c>` | `(a, b)<c>;` | `TestParenthesizeExpressionWithTypeArguments` | |
| `parenthesize_as_expression` | `(a, b) as c` | `(a, b) as c;` | `TestParenthesizeAsExpression` | |
| `parenthesize_satisfies_expression` | `(a, b) satisfies c` | `(a, b) satisfies c;` | `TestParenthesizeSatisfiesExpression` | |
| `parenthesize_non_null_expression` | `(a, b)!` | `(a, b)!;` | `TestParenthesizeNonNullExpression` | |
| `parenthesize_expression_statement_1` | 语句首对象字面量加括号 | `({});` | `TestParenthesizeExpressionStatement1` | |
| `parenthesize_expression_statement_2` | 语句首函数表达式加括号 | `(function () { });` | `TestParenthesizeExpressionStatement2` | |
| `parenthesize_expression_statement_3` | 语句首 class 表达式 | `class {\n};` | `TestParenthesizeExpressionStatement3` | |
| `parenthesize_expression_default_1` | export default class | `export default (class {\n});` | `TestParenthesizeExpressionDefault1` | |
| `parenthesize_expression_default_2` | export default function | `export default (function () { });` | `TestParenthesizeExpressionDefault2` | |
| `parenthesize_expression_default_3` | export default 逗号 | `export default (a, b);` | `TestParenthesizeExpressionDefault3` | |

### 类型 parenthesizer（单场景）

| Rust 测试 | 验证内容 | expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parenthesize_array_type` | 联合类型数组 | `type _ = (a \| b)[];` | `TestParenthesizeArrayType` | |
| `parenthesize_optional_type` | tuple 内可选联合 | `type _ = [\n    (a \| b)?\n];` | `TestParenthesizeOptionalType` | |
| `parenthesize_union_type_1` | 联合含函数类型 | `type _ = a \| (() => b);` | `TestParenthesizeUnionType1` | |
| `parenthesize_union_type_2` | 联合含 infer extends | `type _ = (infer a extends b) \| c;` | `TestParenthesizeUnionType2` | |
| `parenthesize_intersection_type` | 交叉含联合 | `type _ = a & (b \| c);` | `TestParenthesizeIntersectionType` | |
| `parenthesize_readonly_type_operator_1` | readonly (联合) | `type _ = readonly (a \| b);` | `TestParenthesizeReadonlyTypeOperator1` | |
| `parenthesize_readonly_type_operator_2` | readonly (keyof) | `type _ = readonly (keyof a);` | `TestParenthesizeReadonlyTypeOperator2` | |
| `parenthesize_keyof_type_operator` | keyof (联合) | `type _ = keyof (a \| b);` | `TestParenthesizeKeyofTypeOperator` | |
| `parenthesize_indexed_access_type` | (联合)[c] | `type _ = (a \| b)[c];` | `TestParenthesizeIndexedAccessType` | |
| `parenthesize_conditional_type_1` | (函数类型) extends | `type _ = (() => a) extends b ? c : d;` | `TestParenthesizeConditionalType1` | |
| `parenthesize_conditional_type_2` | extends 嵌套条件类型加括号 | `type _ = a extends (b extends c ? d : e) ? f : g;` | `TestParenthesizeConditionalType2` | |
| `parenthesize_conditional_type_3` | infer extends 在 true 分支 | `type _ = a extends () => (infer b extends c) ? d : e;` | `TestParenthesizeConditionalType3` | |
| `parenthesize_conditional_type_4` | infer extends 联合 | `type _ = a extends () => (infer b extends c) \| d ? e : f;` | `TestParenthesizeConditionalType4` | |

### `TestParenthesizeBinaryExpressionMixingNullishCoalescing`（表驱动，8 子用例）

| Rust 测试 | (inner, outer, side) → output | Go 对照 | 完成 |
|---|---|---|---|
| `nullish_bar_bar_left_qq` | `??` 在 `\|\|` 左 → `(a ?? b) \|\| c;` | `.../BarBarWithLeftQuestionQuestion` | |
| `nullish_amp_amp_left_qq` | `??` 在 `&&` 左 → `(a ?? b) && c;` | `.../AmpersandAmpersandWithLeftQuestionQuestion` | |
| `nullish_bar_bar_right_qq` | `??` 在 `\|\|` 右 → `a \|\| (b ?? c);` | `.../BarBarWithRightQuestionQuestion` | |
| `nullish_amp_amp_right_qq` | `??` 在 `&&` 右 → `a && (b ?? c);` | `.../AmpersandAmpersandWithRightQuestionQuestion` | |
| `nullish_qq_left_bar_bar` | `\|\|` 在 `??` 左 → `(a \|\| b) ?? c;` | `.../QuestionQuestionWithLeftBarBar` | |
| `nullish_qq_left_amp_amp` | `&&` 在 `??` 左 → `(a && b) ?? c;` | `.../QuestionQuestionWithLeftAmpersandAmpersand` | |
| `nullish_qq_right_bar_bar` | `\|\|` 在 `??` 右 → `a ?? (b \|\| c);` | `.../QuestionQuestionWithRightBarBar` | |
| `nullish_qq_right_amp_amp` | `&&` 在 `??` 右 → `a ?? (b && c);` | `.../QuestionQuestionWithRightAmpersandAmpersand` | |

### 名字生成 + transform 后 emit（单场景，4 func）

| Rust 测试 | 验证内容 | expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `name_generation` | temp 变量在嵌套函数作用域内各得 `_a` | `var _a;\nfunction f() {\n    var _a;\n}` | `TestNameGeneration` | |
| `no_trailing_comma_after_transform` | NonNull 剥除后无尾逗号 | `[a!]` 经 visitor 去 NonNull → `[a];` | `TestNoTrailingCommaAfterTransform` | |
| `trailing_comma_after_transform` | 保留显式尾逗号 | `[a!,]` → `[a,];` | `TestTrailingCommaAfterTransform` | |
| `partially_emitted_expression` | TypeEraser 后保留多行属性链布局 | `return ((container.parent\n    .left as ...)... ` → `return container.parent\n    .left\n    .expression\n    .expression;` | `TestPartiallyEmittedExpression` | |

---

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*`（105 个）都已映射（utilities 4 + namegenerator 36 + printer 65）
- [ ] `TestEmit` 的 ~290 表驱动子用例逐 NodeKind 全列；impl.md 的 `emit_<Kind>` TODO 覆盖每个出现的 NodeKind
- [ ] `TestEscape*` / `TestIsRecognizedTripleSlashComment` 子用例逐条列出，对应 `utilities.rs` 的 escape/triple-slash TODO
- [ ] 全部 `TestParenthesize*` 对应 `printer.rs` 的 `parenthesize_*` TODO
- [ ] `namegenerator_test.go` 36 条对应 `namegenerator.rs` + `factory.rs` 名字工厂 TODO
- [ ] expected 值均取自 Go 测试字面量（非 Rust 推断）
- [ ] 每条带 `// Go:` 锚点（`<file>_test.go:<TestFunc>[/<case>]`）
- [ ] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 完整 emit conformance / fourslash baseline | 需 program + 全量 fixtures | P10 |
| source map 字节级 emit 对拍（`--sourceMap`） | 需真实编译输出 | P10 |
| `.d.ts` 声明 emit 集成 | 依赖 transformers/declarations + checker resolver | P5（transformers 包）/ P10 |
| `ChangeTrackerWriter` 经 format/ls 的使用 | 上游在语言服务 | P7 |
