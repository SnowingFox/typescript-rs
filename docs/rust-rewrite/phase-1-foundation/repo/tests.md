# repo: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：0 文件 / 0 `func Test` / 0 子用例。

## 0 直接单测的情况

- Go 侧无 `*_test.go`：`internal/repo` 是测试基础设施（被其它测试 import），自身**无直接单测**；其正确性由"依赖它的测试能找到 testdata / 子模块"间接保证，归 **P10 conformance/fourslash parity**。
- 本轮补充的行为级 Rust 测试（基于路径拼接的确定性 + 磁盘实况）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `root_path_contains_workspace_manifest` | 仓库根目录存在 workspace `Cargo.toml` | `Path::new(root_path()).join("Cargo.toml").exists()` → true | paths.go:RootPath | |
| `root_path_is_absolute` | 返回绝对路径 | `Path::new(root_path()).is_absolute()` → true | paths.go:rootPath（IsAbs 断言） | |
| `submodule_path_suffix` | 子模块路径后缀正确 | `typescript_submodule_path()` 以 `_submodules/TypeScript` 结尾 | paths.go:TypeScriptSubmodulePath | |
| `test_data_path_suffix` | testdata 路径后缀正确 | `test_data_path()` 以 `testdata` 结尾 | paths.go:TestDataPath | |
| `submodule_exists_matches_disk` | 存在性与磁盘一致 | `typescript_submodule_exists()` == `<submodule>/package.json` 实际存在 | paths.go:TypeScriptSubmoduleExists | |
| `skip_helper_no_panic` | skip 辅助不 panic | 调用 `skip_if_no_typescript_submodule()` 不 panic（返回布尔/跳过） | paths.go:SkipIfNoTypeScriptSubmodule | |

## 与 impl.md 的对齐核对

- [ ] 每条行为级用例带 `// Go:` 锚（「依据」列即上游锚，指向实现源 `internal/repo/<file>.go:<Func>`，因 Go 侧无 `*_test.go`）

- [x] 无 Go `func Test*` 需映射（0 直接单测，已说明）
- [x] 补充测试覆盖 root_path / submodule_path / test_data_path / exists / skip，均在 impl.md 有 TODO 承载
- [x] expected 取自路径拼接确定性与磁盘实况
- [x] 每条补充测试标注依据

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 依赖 testdata / 子模块的 conformance 测试真正运行 | 需 P10 测试设施 | P10 parity |
| `-trimpath`/特殊构建下的定位行为 | Rust 用 manifest dir，分支不同，按需评估 | 实现期 |
