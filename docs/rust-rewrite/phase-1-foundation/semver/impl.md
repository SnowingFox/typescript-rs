# semver: 实现方案（impl.md）

**crate**：`tsgo_semver`　**目标**：实现 semver 版本（`Version`）与 npm 风格版本范围（`VersionRange`，含 `~`/`^`/`-`/`||`/通配符）的解析、比较、匹配。
**依赖（crate）**：无内部 crate。外部：`regex`。
**Go 源**：`internal/semver/`（2 个非测试文件：`version.go` 275、`version_range.go` 439 行）

## 这个包是什么（业务说明）

TypeScript 用 semver 处理 `typesVersions`、`engines`、`@types` 包匹配、`typescript` 版本约束等。本包是 npm `node-semver` 的子集实现：

- **`Version`**（`version.go`）：`major.minor.patch[-prerelease][+build]`，允许缺省 minor/patch（默认 0，这是相对官方 semver 的有意放宽）。`Compare` 严格按 semver §11 优先级（major/minor/patch 数值比较 → prerelease 逐标识符比较，数字标识符 < 非数字、build 不参与优先级）。
- **`VersionRange`**（`version_range.go`）：npm range 语法（`range-set ::= range (|| range)*`），支持 primitive（`< <= = >= >`）、partial、tilde(`~`)、caret(`^`)、hyphen(`a - b`)、通配符(`x`/`X`/`*`)。`Test(version)` 判定版本是否落在范围内。解析把每种 range 展开为 `>=`/`<` 等比较器的合取（conjunction），多个 range 用 `||` 析取（disjunction）。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Version{major,minor,patch uint32; prerelease,build []string}` | `struct Version{ major:u32, minor:u32, patch:u32, prerelease:Vec<String>, build:Vec<String> }` | 字段全私有（Go 也是小写）；构造经 `TryParseVersion` |
| `regexp.MustCompile`（6 个版本正则 + 5 个 range 正则） | `regex::Regex`（`LazyLock<Regex>`） | 正则直接照搬（`(?i)` → `(?i)`）；用 `LazyLock` 对齐包级 `var` |
| `Compare(b) int`（-1/0/1） | `impl Ord for Version` / `fn compare(&self,&Self)->Ordering` | Go 用 `int`，Rust 用 `Ordering`；nil 比较（`a==b`/`a==nil`）→ Rust 无 nil，`Compare` 直接对 `&Version` |
| `cmp.Compare` / `slices.CompareFunc` | `u32::cmp` / `Iterator::cmp`/手写 | prerelease 逐标识符比较用 `slices.CompareFunc(comparePreReleaseIdentifier)` |
| `comparatorOperator string`（`< <= = >= >`） | `enum ComparatorOperator { Lt, Le, Eq, Ge, Gt }` | Go 用字符串常量；Rust 用 enum + `Display` 还原符号 |
| `VersionRange{alternatives [][]versionComparator}` | `struct VersionRange{ alternatives: Vec<Vec<VersionComparator>> }` | 析取的合取（OR of ANDs） |
| `(Version, error)` / `(T, bool)` | `Result<Version, SemverParseError>` / `Option<...>` | `TryParseVersion`→Result；`TryParseVersionRange`→`(VersionRange, bool)` 可用 `Option<VersionRange>` |
| `SemverParseError{origInput}` | `struct SemverParseError{ orig_input: String }`（`thiserror`） | `Error()` 消息 `Could not parse version string from %q` 须保留 |
| `strconv.ParseUint(_,10,32)` | `u32::from_str` | `getUintComponent`；溢出在 prerelease 数字比较里特判 |
| `panic`（MustParse / 未知 operator） | `panic!` | 同语义 |

> **注意**：`prereleasePartRegexp` 与 `buildPartRegExp` 在 `version.go` 声明但当前代码未引用（疑似预留/历史遗留）。移植时可先省略或保留为常量，标 `// TODO(port)` 确认是否需要。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/semver/version.go` | `internal/semver/version.rs`（在 `lib.rs` 里 `mod version; pub use version::*;`） | Version + 解析 + 比较 |
| `internal/semver/version_range.go` | `internal/semver/version_range.rs` | VersionRange + 解析 + Test |
| （crate 根） | `internal/semver/lib.rs` | 声明子模块 + re-export |

## 依赖白名单（本包新增的 crate）

- `regex`（版本/范围语法正则）。
- `thiserror`（`SemverParseError`）。
- 记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `version.rs`（Go: `internal/semver/version.go`）

- [ ] 正则常量：`version_regexp` / `prerelease_regexp` / `build_regexp` / `numeric_identifier_regexp`（`prerelease_part`/`build_part` 见上注，待确认）　`// Go: version.go`（var 块）
- [ ] `pub struct Version{major,minor,patch:u32, prerelease,build:Vec<String>}` + `version_zero`（prerelease=["0"]）　`// Go: version.go:Version/versionZero`
- [ ] `fn increment_major/minor/patch(&self) -> Version`　`// Go: version.go:incrementMajor/incrementMinor/incrementPatch`
- [ ] `fn compare(&self, b: &Version) -> Ordering`（major→minor→patch→prerelease）　`// Go: version.go:Compare`
- [ ] `fn compare_prerelease_identifiers(left, right) -> Ordering`（空集语义：有 prerelease 优先级更低）　`// Go: version.go:comparePreReleaseIdentifiers`
- [ ] `fn compare_prerelease_identifier(left, right) -> Ordering`（数字<非数字；数字按数值；溢出回退长度/字符串；字母按 ASCII）　`// Go: version.go:comparePreReleaseIdentifier`
- [ ] `impl Display for Version`（`maj.min.patch[-pre][+build]`）　`// Go: version.go:(Version).String`
- [ ] `struct SemverParseError{orig_input}` + `Display`（消息 `Could not parse version string from "..."`）　`// Go: version.go:SemverParseError/Error`
- [ ] `pub fn try_parse_version(text) -> Result<Version, SemverParseError>`（正则提取 + 各段校验）　`// Go: version.go:TryParseVersion`
- [ ] `pub fn must_parse(text) -> Version`（失败 panic）　`// Go: version.go:MustParse`
- [ ] `fn get_uint_component(text) -> Result<u32, _>`　`// Go: version.go:getUintComponent`

### `version_range.rs`（Go: `internal/semver/version_range.go`）

- [ ] 正则常量：`logical_or` / `whitespace` / `partial` / `hyphen` / `range`　`// Go: version_range.go`（var 块）
- [ ] `pub struct VersionRange{alternatives: Vec<Vec<VersionComparator>>}` + `struct VersionComparator{operator, operand}` + `enum ComparatorOperator{Lt,Le,Eq,Ge,Gt}`　`// Go: version_range.go:VersionRange/versionComparator/comparatorOperator`
- [ ] `impl Display for VersionRange`（`format_disjunction`：空→`*`、`format_alternative`/`format_comparator`）　`// Go: version_range.go:String/formatDisjunction/formatAlternative/formatComparator`
- [ ] `fn test(&self, version: &Version) -> bool`（`test_disjunction`：空→true 全匹配 / `test_alternative` / `test_comparator`）　`// Go: version_range.go:Test/testDisjunction/testAlternative/testComparator`
- [ ] `pub fn try_parse_version_range(text) -> (VersionRange, bool)`（`parse_alternatives`：按 `||` 拆 → hyphen 或空白拆 simple）　`// Go: version_range.go:TryParseVersionRange/parseAlternatives`
- [ ] `fn parse_hyphen(left, right) -> Option<Vec<VersionComparator>>`（左非通配→`>=`；右按 minor/patch 通配展开 `<`/`<=`）　`// Go: version_range.go:parseHyphen`
- [ ] `struct PartialVersion{version, major_str, minor_str, patch_str}` + `fn parse_partial(text) -> Option<PartialVersion>`（通配符 `x/X/*` 处理）　`// Go: version_range.go:partialVersion/parsePartial`
- [ ] `fn parse_comparator(op, text) -> Option<Vec<VersionComparator>>`（`~`/`^`/`< >=`/`<= >`/`= ""` 各分支展开；通配 major 时 `<`/`>` → `<0.0.0-0`）　`// Go: version_range.go:parseComparator`
- [ ] `fn is_wildcard(text) -> bool`（`* x X`）　`// Go: version_range.go:isWildcard`

### Cargo / crate 接线

- [ ] `internal/semver/Cargo.toml`（`name = "tsgo_semver"`，deps `regex` `thiserror`）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明子模块 + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `try_parse_version` + `Display`（`TestTryParseSemver` 4 + `TestVersionString` 6）—— tracer bullet。
2. `compare`（`TestVersionCompare` ~37：major/minor/patch/prerelease/build/数值/字典序/大数）。
3. `parse_partial` + `is_wildcard` + `VersionRange::Display`（`TestWildcardsHaveSameString`：同义通配符回显一致）。
4. `parse_comparator`（primitive/`~`/`^`）+ `test`（`TestVersionRanges` good/bad + `TestTildes*`/`TestCarets*`）。
5. `parse_hyphen`（`TestHyphens*`）+ 合取/析取（`TestConjunctions*`/`TestDisjunctions*`）。
6. **`TestComparatorsOfVersionRanges`**（~340 子用例的全运算符 × 版本矩阵，最终 gate）。

## 与 Go 的已知偏离（divergence）

- **缺省 minor/patch**：本包有意放宽官方 semver，允许 `1`、`1.2`（缺省段=0）。Rust 照搬，**不**用现成 `semver` crate（其严格语义不同），自写解析。
- **`Compare` 的 nil 分支**：Go `Compare` 处理 `a==b`/`a==nil`/`b==nil`。Rust 无 nil，`compare` 接 `&Version`；nil 语义若上层需要（如可空版本）用 `Option<Version>` 在调用点处理。
- **`comparatorOperator` 字符串 → enum**：`Display` 必须还原 `< <= = >= >` 原符号（`VersionRange::String` 测试依赖）。
- **大数 prerelease**：`comparePreReleaseIdentifier` 对超 `u32`/`u64` 的纯数字标识符回退到长度比较再字符串比较（`TestVersionRanges` 的 `lotsaOnes` 320 位用例）。Rust 用 `u64::from_str` 失败时同样回退。
- **正则引擎差异**：Go `regexp`(RE2) vs Rust `regex`(也是 RE2 系)，语法基本一致；`(?i)` 大小写不敏感保留。

## 转交 / 推迟（DEFER）

- `prereleasePartRegexp` / `buildPartRegExp` 未引用，待实现期确认是否删除（`// TODO(port)`）。
- 本包真实用于 `typesVersions`/`@types` 匹配在 module/program（P4/P6）接通；本包 P1 完整实现并测。
