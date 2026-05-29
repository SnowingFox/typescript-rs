# module: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 个测试文件 / 2 个 `func Test*` / 均为单块（非表驱动），都是 issue #3526 的回归测试。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/module/resolver_test.go` | `internal/module/tests/resolver.rs`（集成测试，跨包用 `module::` 公开 API + `vfstest`） | 2 |

> 这两个测试是 `package module_test`（外部测试包），用真实 `module.NewResolver` + 内存 vfs，属集成性质 → 放 `tests/` 目录。

## `resolver_test.go`

### `TestResolveModuleNameTrailingSlash`

> Go: 解析 `"pkg"` 与 `"pkg/"`（拖尾斜杠）必须得到相同的"已解析"结果。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `resolve_module_name_trailing_slash` | `pkg` 与 `pkg/` 都能解析成功 | vfs: `/repo/node_modules/pkg/{package.json(main+types),main.d.ts,main.js}` + `/repo/src/file.ts`；opts: `Bundler`/`ESNext`；对 `["pkg","pkg/"]` 各 `resolve_module_name(name,"/repo/src/file.ts",ESNext,None)` → 两者 `is_resolved()==true` | `resolver_test.go:TestResolveModuleNameTrailingSlash` | |

### `TestResolveModuleNameTrailingSlashRace`

> Go: 两个线程分别解析 `pkg` / `pkg/`，用 blockingFS 在 `package.json` 的 `FileExists` 处卡住两者（保证都已观察到 info-cache miss），释放后两者都走 `LoadOrStore`、其中一个"落败"。若 candidate 规范化缺失，落败者会拿到不匹配的 entry、跳过 `types` 加载 → 幻觉 TS2307。该测试在还原修复前必然失败。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `resolve_module_name_trailing_slash_race` | 并发竞态下 `pkg`/`pkg/` 仍都解析成功 | vfs: `/repo/node_modules/pkg/package.json`(只有 `types:"./typings/index.d.ts"`，无 main/index) + `typings/index.d.ts` + `/repo/src/a/file.ts`、`/repo/src/b/file.ts`；用 `BlockingFs`(包裹 `vfstest::from_map`，在 `package.json` 的 `file_exists` 阻塞并计数) 让两线程都到 gate 后释放；`std::thread::scope` 两线程各解析 → 两者 `is_resolved()==true` | `resolver_test.go:TestResolveModuleNameTrailingSlashRace` | |

> 实现细节（Rust 侧测试基建）：
> - `BlockingFs` 实现 `Vfs`，转发给内层；`file_exists(path)==target` 时 `waiting.fetch_add(1)` 并阻塞在一个 `Barrier`/`channel`，主线程轮询 `waiting>=2` 后释放。
> - `std::thread::scope` + 每线程独立 containing file（保证 module-resolution-cache key 不同），结果经 `channel`/`Mutex<Vec>` 收集。
> - 这是 PORTING §6 "真并发 + 确定性断言"的范例：断言只看"两者都解析成功"，不依赖线程调度顺序。

## 0 直接单测的文件（补充行为级 Rust 测试）

`util.go`（纯字符串算法）在 Go 侧无独立单测，但被 conformance 重度覆盖。补行为级单测（expected 取自 Go 实现语义）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `parse_package_name_plain` | 普通包名 | `"foo/bar/baz"` → `("foo","bar/baz")` | util.go:ParsePackageName | |
| `parse_package_name_scoped` | scope 包名 | `"@a/b/c"` → `("@a/b","c")` | util.go:ParsePackageName | |
| `parse_package_name_no_slash` | 无斜杠 | `"foo"` → `("foo","")` | util.go:ParsePackageName | |
| `mangle_scoped` | scope 名编码 | `"@a/b"` → `"a__b"` | util.go:MangleScopedPackageName | |
| `mangle_non_scoped` | 非 scope 原样 | `"foo"` → `"foo"` | util.go:MangleScopedPackageName | |
| `unmangle_scoped` | 解码 | `"a__b"` → `"@a/b"` | util.go:UnmangleScopedPackageName | |
| `types_package_name` | @types 名 | `"@a/b"` → `"@types/a__b"` | util.go:GetTypesPackageName | |
| `pkg_name_from_types` | 从 @types 反推 | `"@types/a__b"` → `"@a/b"`; `"foo"`→`"foo"` | util.go:GetPackageNameFromTypesPackageName | |
| `compare_pattern_keys_base_len` | base 长者优先(-1) | `("a/*","ab/*")` → `1`（ab base 更长）；`("ab/*","a/*")`→`-1` | util.go:ComparePatternKeys | |
| `compare_pattern_keys_no_star` | 无 `*` 规则 | `("a","a/*")` 同 base → 含 `*` 者优先 | util.go:ComparePatternKeys | |
| `parse_node_module_from_path` | 取 node_modules 包目录 | `"/x/node_modules/pkg/a.js"`,folder=false → `"/x/node_modules/pkg"` | util.go:ParseNodeModuleFromPath | |
| `parse_node_module_scoped` | scope 包 | `"/x/node_modules/@s/pkg/a.js"` → `"/x/node_modules/@s/pkg"` | util.go:ParseNodeModuleFromPath | |
| `try_get_js_extension_ts` | `.ts`→`.js` | `"a.ts"` → `".js"` | util.go:TryGetJSExtensionForFile | |
| `try_get_js_extension_mts` | `.mts`→`.mjs` | `"a.mts"` → `".mjs"` | util.go:TryGetJSExtensionForFile | |
| `try_get_js_extension_tsx_preserve` | `.tsx` + jsx=preserve → `.jsx` | `"a.tsx"`,jsx=Preserve → `".jsx"` | util.go:TryGetJSExtensionForFile | |
| `is_applicable_versioned_types_key` | `types@<range>` 命中当前 TS 版本 | `"types@>=1.0"` → `true`；`"foo"`→`false` | util.go:IsApplicableVersionedTypesKey | |

> `cache.go`/`types.go` 的 bitflags 位值快照（`NodeResolutionFeatures`/`extensions`）也补一个 `*_bit_values` 测试。

## 与 impl.md 的对齐核对

- [ ] 每个 Go `func Test*`（2 个回归）都已映射
- [ ] 表驱动子用例 —— N/A（两测试均单块）
- [ ] expected 值均取自 Go 测试字面量 / util 实现语义
- [ ] 每条带 `// Go:` 锚点
- [ ] 与 impl.md 双向对齐：`resolve_module_name` + candidate 规范化 + `InfoCache.LoadOrStore` 实现 TODO 承载这两个回归测试

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `exports`/`imports`/`paths`/`rootDirs`/`typeRoots`/`typesVersions`/`@types` 全量正确性 | 规则海量，Go 侧也靠 conformance | P10（`conformance/moduleResolution/*`） |
| `node16`/`nodenext`/`bundler` 条件矩阵 | 端到端 | P10 |
| 性能（大型 node_modules 树） | 性能对拍 | P10 |
