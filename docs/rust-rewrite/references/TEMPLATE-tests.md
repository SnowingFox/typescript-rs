# <pkg>: 测试清单（tests.md 模板）

> 复制本模板到 `phase-N-*/<pkg>/tests.md`。删除本行与 `<...>` 占位。
> **必须实际读** `internal/<pkg>/*_test.go` 每个文件，逐 `func Test*`、逐表驱动子用例对齐。
> 这是单测 1:1 复原的核心产物 —— 漏一个子用例就是漏一条行为。

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：<测试文件数> 文件 / <func Test 数> 顶层函数 / 约 <子用例数> 子用例。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试文件（独立，镜像 `_test.go`） | 顶层测试函数数 |
|---|---|---|
| `internal/<pkg>/<file>_test.go` | `internal/<pkg>/<file>_test.rs`（兄弟文件，`use super::*;`，由 `<file>.rs` 末尾 `#[cfg(test)] #[path="<file>_test.rs"] mod tests;` 挂载） | <N> |

## `<file>_test.go`

> Go: `func Test<Xxx>`（表驱动，子用例 `t.Run(tt.name, ...)`）。逐子用例列。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `<test_name>` | <这条断言什么行为> | `<in>` → `<out>` | `<file>_test.go:Test<Xxx>/<case>` | |
| ... | | | | |

<对每个 Go 测试文件重复上面一节。>

## 0 直接单测的情况（如适用）

<若 Go 侧该包无 `*_test.go`：>
- Go 侧无直接单测；该包行为由 **P10 conformance/fourslash parity** 兜底。
- 本轮补充的行为级 Rust 测试（基于公开接口，expected 取自 Go 实测 / TS spec 已知值）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| ... | | | | |

## 与 impl.md 的对齐核对

<勾选确认：tests.md 每条用例都有 impl.md 的实现 TODO 承载；impl.md 每个有 Go 测试的公开函数都在此有对应行。>

- [ ] 每个 Go `func Test*` 都已映射
- [ ] 每个表驱动子用例都已逐行列出
- [ ] expected 值均取自 Go 测试字面量（非 Rust 推断）
- [ ] 每条带 `// Go:` 锚点
- [ ] 与 impl.md 双向对齐无遗漏

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| ... | | |
