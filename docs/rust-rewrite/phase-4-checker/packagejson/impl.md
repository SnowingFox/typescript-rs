# packagejson: 实现方案（impl.md）

**crate**：`tsgo_packagejson`　**目标**：`package.json` 的强类型解析与查询——既保留字段的"原始 JSON 类型 vs 期望类型"以便诊断（`Expected[T]`），又提供 `exports`/`imports`/`typesVersions` 的结构化访问、版本路径映射与按目录缓存。
**依赖（crate）**：`tsgo_collections`（OrderedMap/SyncMap/Set） `tsgo_core` `tsgo_diagnostics` `tsgo_json` `tsgo_semver` `tsgo_tspath`
**Go 源**：`internal/packagejson/`（6 个非测试文件，约 460 行）

## 这个包是什么（业务说明）

模块解析（`module`）、自动导入说明符（`modulespecifiers`）都需要读 `package.json`。但 `package.json` 是用户写的、可能字段类型不对（`"version": 2` 而不是字符串）、可能缺字段、可能有重复键。这个包负责把它解析成结构化数据，并**完整保留"实际 JSON 类型 vs 期望类型"信息**，让 resolver 在字段类型错误时能给出准确诊断（"Expected type of 'main' field in package.json to be 'string', got 'number'"）。

核心抽象有三层：
1. `Expected[T]`：一个字段的"期望类型"包装。即使 JSON 里类型不对也能记录下来（`Valid` / `Null` / `actualJSONType`）。
2. `JSONValue`：一个**保留原始 JSON 形状**的动态值（null/string/number/bool/array/object），object 用 `OrderedMap` 保插入序（决定遍历/诊断顺序）。`ExportsOrImports` 在它之上加 `objectKind`（subpaths/conditions/imports 判别）。
3. `PackageJson` / `InfoCache`：完整字段集合 + 惰性 `typesVersions` 解析（一次性、记录 trace 诊断）+ 按 `tspath.Path` 的并发缓存。

它在 Phase 4 最前段，因为 `module` 直接 import 它。

## 所有权 / 类型映射（本包关键决策）

通用规则见 PORTING.md §3/§5。本包关键决策：

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `Expected[T any]` 泛型 + 自定义 `UnmarshalJSON` | `struct Expected<T> { actual_json_type: String, null: bool, valid: bool, value: T }` + serde 自定义 `Deserialize` | `T` 仅取 `String` / `bool` / `map[string]string` 等具体类型；泛型直译 |
| `JSONValue{ Type; Value any }` | `enum JsonValue { NotPresent, Null, Str(String), Num(f64), Bool(bool), Array(Vec<JsonValue>), Object(OrderedMap<String, JsonValue>) }` | **用判别枚举取代 `Type`+`any`**（PORTING §3）。Go 的 `JSONValueType` 退化为枚举判别。注意 Go `number` 解到 `float64` |
| `OrderedMap[string, JSONValue]`（object） | `indexmap::IndexMap<String, JsonValue>`（封装为 `tsgo_collections::OrderedMap`） | **必须保插入序**：`typesVersions`/`exports` 遍历顺序影响诊断与解析（PORTING §3） |
| `ExportsOrImports{ JSONValue; objectKind }` | `struct ExportsOrImports { value: JsonValue, object_kind: Cell<ObjectKind> }` | `objectKind` 是惰性初始化（`initObjectKind` 改 receiver）。Go 用值 receiver + 内部赋值（其实只改本地副本！见"已知偏离"）。Rust 用 `Cell`/`OnceCell` 缓存 |
| `PackageJson{ ...; once sync.Once; versionTraces }` | `struct PackageJson { fields: Fields, parseable: bool, version_paths: OnceCell<VersionPaths>, version_traces: Vec<DiagnosticAndArgs> }` | `sync.Once` → `OnceCell`/`OnceLock`；`GetVersionPaths` 的惰性 + trace 重放语义保留 |
| `InfoCache{ cache SyncMap[Path, *Entry] }` | `struct InfoCache { cache: DashMap<Path, Arc<InfoCacheEntry>>, current_directory, use_case_sensitive_file_names }` | `collections.SyncMap` → `dashmap`；`LoadOrStore` → `entry().or_insert` |
| 内嵌 `HeaderFields` / `PathFields` / `DependencyFields` 到 `Fields` | `Fields` 用组合（3 个具名子 struct 字段）+ serde `#[serde(flatten)]` | Go 的 struct embedding → 组合 + flatten（PORTING §4 嵌入→组合） |
| `reflect.TypeFor[T]().Kind()`（`ExpectedJSONType`） | 给每个具体 `T` 实现 `trait ExpectedJsonType { fn expected_json_type() -> &'static str }` | 无反射；用 trait 静态分派覆盖 string/bool/array/object/number |
| `json.AllowDuplicateNames(true)` | serde 配置允许重复键（后值覆盖前值） | `Parse` 行为：重复键保留最后一个（见测试 `duplicate names`） |

### JSON 解析策略

Go 这里用的是仓库自带的 `internal/json`（v2 风格 decoder，`UnmarshalerFrom`/`Decoder.PeekKind`）。Rust 侧两条路线：
- 默认用 `serde` + `serde_json`，对 `JsonValue` / `Expected<T>` / `ExportsOrImports` 写自定义 `Deserialize`（镜像 `unmarshalJSONValueV2` 的 `PeekKind` 分派）。
- `tsgo_json`（P1 移植）若已提供等价的流式 decoder，则优先复用以保持 1:1；否则 serde 兜底。该决策在执行期定，记到 `crate-map.md`。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/packagejson/packagejson.go` | `internal/packagejson/lib.rs` | crate 根。`HeaderFields`/`PathFields`/`DependencyFields`/`Fields` + `HasDependency`/`RangeDependencies`/`GetRuntimeDependencyNames` + `Parse` |
| `internal/packagejson/expected.go` | `internal/packagejson/expected.rs` | `Expected<T>` + 自定义反序列化 + `GetValue`/`IsValid`/`IsPresent`/`ExpectedJsonType`/`ActualJsonType`/`ExpectedOf` |
| `internal/packagejson/jsonvalue.go` | `internal/packagejson/jsonvalue.rs` | `JsonValue` 枚举 + `IsPresent`/`IsFalsy`/`AsObject`/`AsArray`/`AsString` + 自定义反序列化（`unmarshalJSONValueV2`） |
| `internal/packagejson/exportsorimports.go` | `internal/packagejson/exportsorimports.rs` | `ExportsOrImports` + `ObjectKind` + `IsSubpaths`/`IsImports`/`IsConditions` + `initObjectKind` |
| `internal/packagejson/cache.go` | `internal/packagejson/cache.rs` | `PackageJson` + `GetVersionPaths`（惰性 + trace）+ `VersionPaths` + `InfoCacheEntry` + `InfoCache` |
| `internal/packagejson/validated.go` | `internal/packagejson/validated.rs` | `TypeValidatedField` trait（`IsPresent`/`IsValid`/`ExpectedJsonType`/`ActualJsonType`） |

## 依赖白名单（本包新增的 crate）

- `serde` / `serde_json`（若不直接复用 `tsgo_json`）——已在 PORTING §10 白名单。
- `indexmap`（经 `tsgo_collections` 间接）。
- 无其它新增；记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `expected.rs`（Go: `expected.go`）

- [ ] `pub struct Expected<T>{ actual_json_type: String, null: bool, valid: bool, value: T }`　`// Go: expected.go:Expected`
- [ ] 自定义 `Deserialize`：`"null"`→`{null:true, actual_json_type:"null"}`；否则尝试解 `value` 成功置 `valid`；按首字节定 `actual_json_type`（`"`→string, `t/f`→boolean, `[`→array, `{`→object, 其余→number）　`// Go: expected.go:UnmarshalJSON`
- [ ] `pub fn is_present(&self) -> bool`（`actual_json_type != ""`）　`// Go: expected.go:IsPresent`
- [ ] `pub fn get_value(&self) -> (T, bool)`（值 + `valid`）　`// Go: expected.go:GetValue`
- [ ] `pub fn is_valid(&self) -> bool`　`// Go: expected.go:IsValid`
- [ ] `fn expected_json_type(&self) -> &str`（经 `ExpectedJsonType` trait 静态分派）　`// Go: expected.go:ExpectedJSONType`
- [ ] `pub fn actual_json_type(&self) -> &str`　`// Go: expected.go:ActualJSONType`
- [ ] `pub fn expected_of<T>(value: T) -> Expected<T>`（构造 valid 实例，`actual_json_type` 取 T 的期望类型）　`// Go: expected.go:ExpectedOf`

### `jsonvalue.rs`（Go: `jsonvalue.go`）

- [ ] `pub enum JsonValue { NotPresent, Null, Str(String), Num(f64), Bool(bool), Array(Vec<JsonValue>), Object(OrderedMap<String, JsonValue>) }`　`// Go: jsonvalue.go:JSONValue`+`JSONValueType`
- [ ] `Display`/`type_str()` for 判别（`null`/`string`/`number`/`boolean`/`array`/`object`/`unknown(n)`）　`// Go: jsonvalue.go:JSONValueType.String`
- [ ] `pub fn is_present(&self) -> bool`　`// Go: jsonvalue.go:IsPresent`
- [ ] `pub fn is_falsy(&self) -> bool`（NotPresent/Null→true；Str("")→true；Num(0)→true；Bool(false)→true；array/object→false）　`// Go: jsonvalue.go:IsFalsy`
- [ ] `pub fn as_object(&self) -> &OrderedMap<String, JsonValue>`（非 object panic）　`// Go: jsonvalue.go:AsObject`
- [ ] `pub fn as_array(&self) -> &[JsonValue]`（非 array panic）　`// Go: jsonvalue.go:AsArray`
- [ ] `pub fn as_string(&self) -> &str`（非 string panic）　`// Go: jsonvalue.go:AsString`
- [ ] 自定义 `Deserialize`（泛型 element `T`，镜像 `unmarshalJSONValueV2` 的 `PeekKind` 分派；array 元素为 `T`、object value 为 `T`）　`// Go: jsonvalue.go:unmarshalJSONValueV2`

### `exportsorimports.rs`（Go: `exportsorimports.go`）

- [ ] `enum ObjectKind { Unknown, Subpaths, Conditions, Imports, Invalid }`　`// Go: exportsorimports.go:objectKind`
- [ ] `pub struct ExportsOrImports { value: JsonValue, object_kind: Cell<ObjectKind> }`　`// Go: exportsorimports.go:ExportsOrImports`
- [ ] 自定义 `Deserialize`（复用 `JsonValue` 解析，element 类型为 `ExportsOrImports`）　`// Go: exportsorimports.go:UnmarshalJSONFrom`
- [ ] `pub fn as_object` / `as_array`（同 JsonValue 但 element 为 `ExportsOrImports`）　`// Go: exportsorimports.go:AsObject/AsArray`
- [ ] `fn init_object_kind(&self)`：object 且非空时按首字符分类——`.`→subpaths、`#`→imports、含其它且混 `.`/`#`→invalid，否则 conditions　`// Go: exportsorimports.go:initObjectKind`
- [ ] `pub fn is_subpaths/is_imports/is_conditions(&self) -> bool`（先 `init_object_kind`）　`// Go: exportsorimports.go:IsSubpaths/IsImports/IsConditions`

### `validated.rs`（Go: `validated.go`）

- [ ] `pub trait TypeValidatedField { fn is_present()->bool; fn is_valid()->bool; fn expected_json_type()->&str; fn actual_json_type()->&str }`，为 `Expected<T>` impl　`// Go: validated.go:TypeValidatedField`

### `lib.rs`（Go: `packagejson.go`）

- [ ] `pub struct HeaderFields { name, version, type_ : Expected<String> }`（`type` → `type_`，serde rename）　`// Go: packagejson.go:HeaderFields`
- [ ] `pub struct PathFields { tsconfig, main, types, typings: Expected<String>, types_versions: JsonValue, imports, exports: ExportsOrImports }`　`// Go: packagejson.go:PathFields`
- [ ] `pub struct DependencyFields { dependencies, dev_dependencies, peer_dependencies, optional_dependencies: Expected<HashMap<String,String>> }`　`// Go: packagejson.go:DependencyFields`
- [ ] `fn has_dependency(&self, name) -> bool`（4 字段任一含 name）　`// Go: packagejson.go:HasDependency`
- [ ] `fn range_dependencies(&self, f)`（遍历 4 字段，回调 `(name, version, field)`，回调返 false 即停）　`// Go: packagejson.go:RangeDependencies`
- [ ] `fn get_runtime_dependency_names(&self) -> Set<String>`（deps+peer+opt，**不含 dev**）　`// Go: packagejson.go:GetRuntimeDependencyNames`
- [ ] `pub struct Fields { header: HeaderFields, path: PathFields, deps: DependencyFields }`（serde flatten）　`// Go: packagejson.go:Fields`
- [ ] `pub fn parse(data: &[u8]) -> Result<Fields, Error>`（允许重复键）　`// Go: packagejson.go:Parse`

### `cache.rs`（Go: `cache.go`）

- [ ] `static TYPESCRIPT_VERSION: semver::Version`（`semver::must_parse(core::version())`）　`// Go: cache.go:typeScriptVersion`
- [ ] `pub struct PackageJson { fields: Fields, parseable: bool, version_paths: OnceCell<VersionPaths>, version_traces: Vec<DiagnosticAndArgs> }`　`// Go: cache.go:PackageJson`
- [ ] `fn get_version_paths(&self, trace) -> VersionPaths`：`OnceCell` 内做 typesVersions 解析（缺字段/类型错/无匹配版本各记 trace），匹配 semver range 命中后取首个；`trace` 非空则重放所有 `version_traces`　`// Go: cache.go:GetVersionPaths`
- [ ] `pub struct VersionPaths { version, paths_json: Option<OrderedMap<..,JsonValue>>, paths: OnceCell<OrderedMap<String, Vec<String>>> }` + `exists()` + `get_paths()`（惰性把 array-of-string 抽出，非 string/array 跳过）　`// Go: cache.go:VersionPaths`
- [ ] `pub struct InfoCacheEntry { package_directory, directory_exists, contents: Option<PackageJson> }` + `exists()`/`get_contents()`/`get_directory()`　`// Go: cache.go:InfoCacheEntry`
- [ ] `pub struct InfoCache { cache: DashMap<Path, Arc<InfoCacheEntry>>, current_directory, use_case_sensitive_file_names }` + `new`/`get`/`set`（key 用 `tspath::to_path`，`set` 用 `LoadOrStore` 语义）　`// Go: cache.go:InfoCache`

### Cargo / crate 接线

- [ ] `internal/packagejson/Cargo.toml`（`name = "tsgo_packagejson"` + path deps）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明 `mod expected; mod jsonvalue; mod exportsorimports; mod validated; mod cache;` + re-export 公开类型

## TDD 推进顺序（tracer bullet → 增量）

1. `JsonValue` 枚举 + 自定义反序列化（先支持 string/number/bool/null/array/object）→ 对应 `TestJSONValue`。
2. `Expected<T>` + 反序列化 → `TestExpected`。
3. `ExportsOrImports` + `init_object_kind` → `TestExports`。
4. `Fields` + `Parse`（含重复键）→ `TestParse`。
5. `cache.rs`（`PackageJson`/`VersionPaths`/`InfoCache`）：Go 无直接单测，补行为级（见 tests.md）。

## 与 Go 的已知偏离（divergence）

- **`initObjectKind` 的 receiver bug-for-bug**：Go 里 `IsSubpaths`/`IsImports`/`IsConditions` 是**值 receiver**，内部调 `e.initObjectKind()`（指针 receiver）改的是**局部副本**的 `objectKind`，所以缓存其实不生效、每次都重算。Rust 用 `Cell<ObjectKind>` 实现真缓存——这是**行为等价但实现更优**的偏离（结果相同，只是不重复计算）。须在 rustdoc 标注。
- `Value any` / `Type` 字段 → `JsonValue` 判别枚举。
- struct embedding → 组合 + serde flatten。
- `reflect` 取期望类型 → trait 静态分派。
- Go number 一律 `float64`（如 `version: 2` → `Num(2.0)`）；Rust 用 `f64`，断言时注意 `2.0`。

## 转交 / 推迟（DEFER）

- `GetVersionPaths` 依赖 `tsgo_semver`（P1）的 `TryParseVersionRange`/`Test` 与 `tsgo_diagnostics`（P2）的诊断消息常量；这些都是已移植 phase，无需推迟。
- benchmark（`BenchmarkPackageJSON`）涉及 `parser.ParseSourceFile`（P3）/ `repo`/`testutil/filefixture`，仅作 P10 性能对拍，不在本包功能单测内。
