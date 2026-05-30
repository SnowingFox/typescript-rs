# module: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：1 个测试文件 / 2 个 `func Test*` / 均为单块（非表驱动），都是 issue #3526 的回归测试。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/module/resolver_test.go` | `internal/module/lib_test.rs`（兄弟单测，用 `crate::` 公开 API + `vfstest::MapFs`） | 2 |

> 偏离：tests.md 原计划放 `tests/resolver.rs`（集成目录），实际放在兄弟 `lib_test.rs`，以满足 C6（每个含 `// Go:` 锚的实现文件须有同目录 `<stem>_test.rs`）。两测试仍走真实 `Resolver::new` + 内存 vfs，集成性质不变；`...Race` 用自写 `BlockingFs`(包裹 `MapFs`) + `std::thread::scope`，断言"两者都解析成功"，不依赖线程调度。

## `resolver_test.go`

### `TestResolveModuleNameTrailingSlash`

> Go: 解析 `"pkg"` 与 `"pkg/"`（拖尾斜杠）必须得到相同的"已解析"结果。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `resolve_module_name_trailing_slash` | `pkg` 与 `pkg/` 都能解析成功 | vfs: `/repo/node_modules/pkg/{package.json(main+types),main.d.ts,main.js}` + `/repo/src/file.ts`；opts: `Bundler`/`ESNext`；对 `["pkg","pkg/"]` 各 `resolve_module_name(name,"/repo/src/file.ts",ESNext,None)` → 两者 `is_resolved()==true` | `resolver_test.go:TestResolveModuleNameTrailingSlash` | ✓ |

### `TestResolveModuleNameTrailingSlashRace`

> Go: 两个线程分别解析 `pkg` / `pkg/`，用 blockingFS 在 `package.json` 的 `FileExists` 处卡住两者（保证都已观察到 info-cache miss），释放后两者都走 `LoadOrStore`、其中一个"落败"。若 candidate 规范化缺失，落败者会拿到不匹配的 entry、跳过 `types` 加载 → 幻觉 TS2307。该测试在还原修复前必然失败。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `resolve_module_name_trailing_slash_race` | 并发竞态下 `pkg`/`pkg/` 仍都解析成功 | vfs: `/repo/node_modules/pkg/package.json`(只有 `types:"./typings/index.d.ts"`，无 main/index) + `typings/index.d.ts` + `/repo/src/a/file.ts`、`/repo/src/b/file.ts`；用 `BlockingFs`(包裹 `vfstest::MapFs`，在 `package.json` 的 `file_exists` 用 `Mutex`+`Condvar` 阻塞并计数) 让两线程都到 gate 后释放；`std::thread::scope` 两线程各解析 → 两者 `is_resolved()==true` | `resolver_test.go:TestResolveModuleNameTrailingSlashRace` | ✓ |

> 实现细节（Rust 侧测试基建）：
> - `BlockingFs` 实现 `Vfs`，转发给内层；`file_exists(path)==target` 时 `waiting.fetch_add(1)` 并阻塞在一个 `Barrier`/`channel`，主线程轮询 `waiting>=2` 后释放。
> - `std::thread::scope` + 每线程独立 containing file（保证 module-resolution-cache key 不同），结果经 `channel`/`Mutex<Vec>` 收集。
> - 这是 PORTING §6 "真并发 + 确定性断言"的范例：断言只看"两者都解析成功"，不依赖线程调度顺序。

## 0 直接单测的文件（补充行为级 Rust 测试）

`util.go`（纯字符串算法）在 Go 侧无独立单测，但被 conformance 重度覆盖。补行为级单测（expected 取自 Go 实现语义）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `parse_package_name_plain` | 普通包名 | `"foo/bar/baz"` → `("foo","bar/baz")` | util.go:ParsePackageName | ✓ |
| `parse_package_name_scoped` | scope 包名 | `"@a/b/c"` → `("@a/b","c")` | util.go:ParsePackageName | ✓ |
| `parse_package_name_no_slash` | 无斜杠 | `"foo"` → `("foo","")` | util.go:ParsePackageName | ✓ |
| `mangle_scoped` | scope 名编码 | `"@a/b"` → `"a__b"` | util.go:MangleScopedPackageName | ✓ |
| `mangle_non_scoped` | 非 scope 原样 | `"foo"` → `"foo"` | util.go:MangleScopedPackageName | ✓ |
| `unmangle_scoped` | 解码 | `"a__b"` → `"@a/b"` | util.go:UnmangleScopedPackageName | ✓ |
| `types_package_name` | @types 名 | `"@a/b"` → `"@types/a__b"` | util.go:GetTypesPackageName | ✓ |
| `pkg_name_from_types` | 从 @types 反推 | `"@types/a__b"` → `"@a/b"`; `"foo"`→`"foo"` | util.go:GetPackageNameFromTypesPackageName | ✓ |
| `compare_pattern_keys_base_len` | base 长者优先(-1) | `("a/*","ab/*")` → `1`（ab base 更长）；`("ab/*","a/*")`→`-1` | util.go:ComparePatternKeys | ✓ |
| `compare_pattern_keys_no_star` | 无 `*` 规则 | `("a","a/*")` 同 base → 含 `*` 者优先 | util.go:ComparePatternKeys | ✓ |
| `parse_node_module_from_path` | 取 node_modules 包目录 | `"/x/node_modules/pkg/a.js"`,folder=false → `"/x/node_modules/pkg"` | util.go:ParseNodeModuleFromPath | ✓ |
| `parse_node_module_scoped` | scope 包 | `"/x/node_modules/@s/pkg/a.js"` → `"/x/node_modules/@s/pkg"` | util.go:ParseNodeModuleFromPath | ✓ |
| `try_get_js_extension_ts` | `.ts`→`.js` | `"a.ts"` → `".js"` | util.go:TryGetJSExtensionForFile | ✓ |
| `try_get_js_extension_mts` | `.mts`→`.mjs` | `"a.mts"` → `".mjs"` | util.go:TryGetJSExtensionForFile | ✓ |
| `try_get_js_extension_tsx_preserve` | `.tsx` + jsx=preserve → `.jsx` | `"a.tsx"`,jsx=Preserve → `".jsx"` | util.go:TryGetJSExtensionForFile | ✓ |
| `is_applicable_versioned_types_key` | `types@<range>` 命中当前 TS 版本 | `"types@>=1.0"` → `true`；`"foo"`→`false` | util.go:IsApplicableVersionedTypesKey | ✓ |

> `cache.go`/`types.go` 的 bitflags 位值快照（`NodeResolutionFeatures`/`extensions`）也补一个 `*_bit_values` 测试。

## §8.6 每函数行为级单测（拆分文件各配 `<stem>_test.rs`，C6）

除上面 2 个回归 + util/types/cache 单测外，按 §8.6"每个 pub fn / 非平凡 helper 都要测"，
为每个拆分文件补了通过公开 `Resolver` API（+ `MapFs` 内存 vfs）驱动该文件代码路径的行为级单测：

| Rust 测试文件 | 覆盖的行为（`// Go:` 锚到对应函数） | 完成 |
|---|---|---|
| `types_test.rs` | `NodeResolutionFeatures`/`Extensions` 位值、`Extensions::String/Array`、`PackageId::package_name/Display`、`Resolved*::is_resolved` | ✓ |
| `cache_test.rs` | `newCaches`、`moduleResolutionCache`/`typeRefDirectiveResolutionCache` get/set、`ModeAwareCache`、`getRedirectConfigName` | ✓ |
| `state_test.rs` | `GetConditions`(bundler/node16/custom)、`getNodeResolutionFeatures`(默认+override)、`resolveNodeLike`(相对)、`GetPackageScopeForPath` | ✓ |
| `file_load_test.rs` | extensionless→`.ts`、`.js`→`.ts`、目录 `index`、`.tsx`/`.jsx`、缺父目录未解析 | ✓ |
| `node_modules_test.rs` | node_modules 包(types)、`@types` 回退、子路径文件、祖先目录上溯 | ✓ |
| `node_resolution_test.rs` | `exports` 子路径、条件 `exports`(types/import)、`null` target 阻断、`#imports` | ✓ |
| `package_info_test.rs` | `main`/`typings` 字段、`peerDependencies`(PackageId 后缀)、无 `package.json` 的 `index.js` | ✓ |
| `paths_test.rs` | `paths` 通配/精确、`rootDirs` 跨目录、相对名跳过 `paths` | ✓ |
| `type_ref_test.rs` | `@types` 类型引用(primary)、未解析、`ResolveConfig`(extends) | ✓ |
| `entrypoints_test.rs` | `types` 入口、`exports` map 入口、目录扫描入口、`Ending`/`SymlinkOrRealpath` | ✓ |
| `lib_test.rs` | 2 个回归 + `GetCompilerOptionsWithRedirect`/`TryParsePatterns`/`MatchPatternOrExact`/`matchesPatternWithTrailer`/`extensionIsOk`/`normalizePathForCJSResolution`/`GetAutomaticTypeDirectiveNames`/相对解析/未解析 | ✓ |

> resolver 内部 helper（`tryLoadInputFileForPath` 的 outDir 回写、`typesVersions`、自身名 self-name、symlink realpath 等深路径）的**完整正确性**仍按 impl.md/§8 推迟到 P10 conformance；本包单测覆盖其主路径与可观察行为。

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*`（2 个回归）都已映射
- [x] 表驱动子用例 —— N/A（两测试均单块）
- [x] expected 值均取自 Go 测试字面量 / util 实现语义
- [x] 每条带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐：`resolve_module_name` + candidate 规范化 + `InfoCache.LoadOrStore` 实现 TODO 承载这两个回归测试

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| `exports`/`imports`/`paths`/`rootDirs`/`typeRoots`/`typesVersions`/`@types` 全量正确性 | 规则海量，Go 侧也靠 conformance | P10（`conformance/moduleResolution/*`） |
| `node16`/`nodenext`/`bundler` 条件矩阵 | 端到端 | P10 |
| 性能（大型 node_modules 树） | 性能对拍 | P10 |
