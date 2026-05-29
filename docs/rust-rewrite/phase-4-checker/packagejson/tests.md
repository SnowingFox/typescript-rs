# packagejson: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：4 个测试文件 / 4 个 `func Test*` + 1 个 `Benchmark` / 共约 4 组用例（`TestParse` 表驱动 1 子用例，其余为单块多断言）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/packagejson/expected_test.go` | `internal/packagejson/expected.rs`（`#[cfg(test)] mod tests`） | 1（`TestExpected`） |
| `internal/packagejson/jsonvalue_test.go` | `internal/packagejson/jsonvalue.rs` | 1（`TestJSONValue` → subtest `UnmarshalJSONV2`） |
| `internal/packagejson/exportsorimports_test.go` | `internal/packagejson/exportsorimports.rs` | 1（`TestExports` → subtest `UnmarshalJSONV2`） |
| `internal/packagejson/packagejson_test.go` | `internal/packagejson/lib.rs` | 1（`TestParse` 表驱动）+ `BenchmarkPackageJSON`（→ P10） |

## `expected_test.go`

> Go: `TestExpected`。单块多断言（非表驱动）：解析 `{"name":"test","version":2,"exports":null}` 到含 4 个 `Expected[...]` 字段的结构。逐断言列。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `expected_name_valid_string` | 合法字符串字段 | `name:"test"` → `valid=true, value="test"` | `expected_test.go:TestExpected` | |
| `expected_version_type_mismatch` | 类型不符（期望 string 得 number） | `version:2` → `valid=false, value=""` | `expected_test.go:TestExpected` | |
| `expected_exports_null` | 显式 null | `exports:null` → `null=true, valid=false` | `expected_test.go:TestExpected` | |
| `expected_main_absent` | 字段缺失 | `main` 不存在 → `valid=false, null=false, value=""` | `expected_test.go:TestExpected` | |

## `jsonvalue_test.go`

> Go: `TestJSONValue` / subtest `UnmarshalJSONV2`（`testJSONValue`）。解析一个含 private/false/name/version/exports/imports/notPresent 的对象，逐断言列。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `jv_bool_true` | `true` → Bool | `private:true` → `Bool(true)` | `jsonvalue_test.go:testJSONValue` | |
| `jv_bool_false` | `false` → Bool | `false:false` → `Bool(false)` | 同 | |
| `jv_string` | 字符串 | `name:"test"` → `Str("test")` | 同 | |
| `jv_number_is_f64` | 数字解为 f64 | `version:2` → `Num(2.0)` | 同（Go `float64(2)`） | |
| `jv_object_size` | object 大小 | `exports` 含 3 键 → `size()==3` | 同 | |
| `jv_nested_object` | 嵌套对象取值 | `exports["."]["import"]` → `Str("./test.ts")` | 同 | |
| `jv_array_type_and_len` | 数组类型与长度 | `exports["./test"]` → Array, `len==3` | 同 | |
| `jv_array_elements` | 数组元素值 | `[0]="./test1.ts"`, `[1]="./test2.ts"`, `[2]=Null` | 同 | |
| `jv_object_null_value` | object 中 null 值 | `exports["./null"]` → `Null` | 同 | |
| `jv_top_level_null` | 顶层 null | `imports:null` → `Null`, value=None | 同 | |
| `jv_not_present` | 缺失字段 | `notPresent` → `NotPresent`, value=None | 同 | |

## `exportsorimports_test.go`

> Go: `TestExports` / subtest `UnmarshalJSONV2`（`testExports`）。解析含 `imports`(#foo) 与 `exports`(./. ...) 的对象，验证 `objectKind` 判别。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `eoi_exports_is_subpaths` | `.`/`./...` 键 → subpaths | `exports` → `is_subpaths()==true` | `exportsorimports_test.go:testExports` | |
| `eoi_exports_size` | 大小 | `exports.size()==3` | 同 | |
| `eoi_dot_is_conditions` | 嵌套条件对象 | `exports["."].is_conditions()==true` | 同 | |
| `eoi_condition_value_string` | 条件值为 string | `exports["."]["import"]` → `Str` | 同 | |
| `eoi_array_null_tail` | 数组尾 null | `exports["./test"][2]` → `Null` | 同 | |
| `eoi_null_subpath` | null 子路径 | `exports["./null"]` → `Null` | 同 | |
| `eoi_imports_is_imports` | `#`-前缀键 → imports | `imports.is_imports()==true` | 同 | |
| `eoi_imports_size` | imports 大小 | `imports.size()==1` | 同 | |
| `eoi_import_foo_is_conditions` | imports 内条件对象 | `imports["#foo"].is_conditions()==true` | 同 | |
| `eoi_import_foo_value_string` | 值为 string | `imports["#foo"]["import"]` → `Str` | 同 | |

## `packagejson_test.go`

> Go: `TestParse`（表驱动，`tests []struct{name, content, want}`，当前 1 子用例）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `parse_duplicate_names` | 重复键保留最后一个，version 正常 | `{"name":"test-package","name":"test-package","version":"1.0.0"}` → `Fields{ name:ExpectedOf("test-package"), version:ExpectedOf("1.0.0") }` | `packagejson_test.go:TestParse/duplicate names` | |

> 注：`assert.DeepEqual` 使用 `cmpopts.IgnoreUnexported`（忽略 `actualJSONType` 等未导出字段）。Rust 侧断言只比 `valid`/`value`（不比 `actual_json_type`），以匹配 Go 的比较语义。

## 0 直接单测的文件（补充行为级 Rust 测试）

`cache.go` / `validated.go` 无 Go 直接单测；行为由 **P10 conformance/module 解析 parity** 兜底。补少量行为级测试（expected 取自 Go 语义）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `version_paths_absent_field` | 缺 typesVersions | 无字段 → `get_version_paths` 返回空 + 记 "does not have a 'typesVersions' field" trace | cache.go:GetVersionPaths | |
| `version_paths_wrong_type` | typesVersions 非对象 | `typesVersions:1` → 空 + 记 "Expected type ... got number" trace | cache.go:GetVersionPaths | |
| `version_paths_match` | 命中版本 range | `{"typesVersions":{">=4.0":{"*":["ts4/*"]}}}` → `VersionPaths.exists()==true`, `get_paths()["*"]==["ts4/*"]` | cache.go:GetVersionPaths/GetPaths | |
| `info_cache_set_get_roundtrip` | 按规范化路径缓存读写 | `set("/p/package.json", e); get(...)` → 同 entry | cache.go:InfoCache | |
| `info_cache_load_or_store` | 并发 set 只保留首个 | 两次 `set` 同 key → 第二次返回首个 | cache.go:Set（LoadOrStore） | |
| `has_dependency_across_fields` | 4 类依赖字段任一命中 | dev-only dep `x` → `has_dependency("x")==true` | packagejson.go:HasDependency | |
| `runtime_deps_excludes_dev` | 运行期依赖不含 dev | deps{a}+dev{b} → `{a}` | packagejson.go:GetRuntimeDependencyNames | |

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*` 都已映射（`TestExpected`/`TestJSONValue`/`TestExports`/`TestParse`）
- [ ] 每个表驱动子用例都已逐行列出（`TestParse/duplicate names`；其余非表驱动按断言拆行）
- [ ] expected 值均取自 Go 测试字面量（注意 number→`f64`，如 `Num(2.0)`）
- [ ] 每条带 `// Go:` 锚点
- [ ] 与 impl.md 双向对齐：`Expected`/`JsonValue`/`ExportsOrImports`/`Parse`/`cache` 实现 TODO 均有用例承载

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `BenchmarkPackageJSON`（含 `ParseJSONText`） | 依赖 `parser`(P3)/`repo`/`testutil` + 仅性能 | P10 |
| 真实 `package.json` fixtures（date-fns.json 等）对拍 | 端到端 | P10 |
