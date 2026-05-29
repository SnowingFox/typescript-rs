# 质量 Gate — 文档 gate + 代码 gate（可运行门禁）

> 把本套移植的「文档纪律」「测试纪律」「Phase 完成纪律」从口头约定**硬化为可运行、可 CI 接入的门禁**。
> 灵感来自 story 仓库的 `audit-deferrals.sh` / `audit-test-go-parity.sh` / `audit-stub-readiness.sh` 三件套
> （复盘见 story `docs/ts-rewrite/impl/references/{testing-discipline,phase-completion-discipline}.md`：
> 「测试全绿 + 91% 覆盖率」仍藏住 25 个 Go 对齐 bug，根因是人工 audit 被跳过 → 必须自动化）。

脚本位于 `docs/rust-rewrite/scripts/`：

| 脚本 | 作用 | 何时跑 |
|---|---|---|
| `gate-docs.sh` | 纯文档检查（bash + ripgrep，不依赖 Rust 代码） | 现在就能跑；每次改文档后 |
| `gate-code.sh` | Rust 代码检查（无 crate 时优雅 no-op） | Cargo workspace 落地后；每个包收口前 |
| `gate.sh` | 聚合入口（`--docs-only` / `--code-only` / `--strict`） | phase 收口前必跑 |

所有脚本：`#!/usr/bin/env bash` + `set -euo pipefail`，用脚本自身路径（回退 `git rev-parse`）定位仓库根，
在任意目录下都能跑。**已 `chmod +x`**。

## 本地运行

```bash
# 文档 gate（只读，安全）
bash docs/rust-rewrite/scripts/gate-docs.sh
bash docs/rust-rewrite/scripts/gate-docs.sh --strict   # WARN 也计入失败

# 代码 gate（无 Cargo.toml 时打印「跳过」并退出 0）
bash docs/rust-rewrite/scripts/gate-code.sh

# 聚合
bash docs/rust-rewrite/scripts/gate.sh                 # 文档 + 代码
bash docs/rust-rewrite/scripts/gate.sh --docs-only
bash docs/rust-rewrite/scripts/gate.sh --code-only
bash docs/rust-rewrite/scripts/gate.sh --strict
```

退出码：`0` = 全绿；`1` = 有 FAIL（含依赖序倒置）；`2` = 用法错误。

---

## A. 文档 gate（`gate-docs.sh`）

> 检查对象：`phase-*/<pkg>/{impl.md,tests.md}`（移植文档）。`README.md` / `PORTING.md` / `references/*.md`
> 是契约文档，不参与内容类检查（D2-D5），仅 `README.md` 被 D6 解析。

| ID | 检查什么 | 为何 | 怎么修 | 级别 |
|---|---|---|---|---|
| **D1** | 每个 `phase-*/<pkg>/` 同时有 `impl.md` + `tests.md`；每个 `phase-*/` 有 `README.md` | 结构缺失 = 该包/该 phase 无法被 TDD 驱动 | 补齐缺的文件（按 `references/TEMPLATE-*.md`） | FAIL |
| **D1b** | `README` phase→包 映射里每个包都有文档目录 | 计划列了包却没文档 = 漏移植 | 建对应 `phase-*/<pkg>/` 目录 | WARN |
| **D2** | 每个 `impl.md`/`tests.md` ≥1 个 `// Go:` 锚；`tests.md` 有 `Go 对照` 或 `依据` 参照列 | 没有 Go 锚 = 失去与上游 1:1 对照的能力（PORTING §7/§8） | 给函数/用例加 `// Go: internal/<pkg>/<file>.go:<Func>`；0 直接单测包用「依据」列指向实现源 | 锚=FAIL，参照列=WARN |
| **D3** | `impl.md`「实现 TODO」段只用 `- [ ]`/`- [x]`，无裸 bullet | 只有 checkbox 才能被 `grep -c "^- \[ \]"` 量化进度，进而 gate 顶层 `[x]` | 把裸 `- xxx` 改成 `- [ ] xxx` | FAIL |
| **D4** | `tests.md` 含 `✓`/`—` 完成列图例 | 完成列语义自解释，避免「留空 vs 推迟」歧义 | 在头部加图例行（见 `TEMPLATE-tests.md`） | FAIL |
| **D5** | Cargo crate 名须 `tsgo` 前缀；代码围栏内禁止 section divider 注释（`// ====` / `/* ===` / `// ----`） | 命名统一（PORTING §2）；divider 是 story 明令禁止的坏味道（PORTING §7.4） | crate 名改 `tsgo_<pkg>`（`cmd/tsgo` bin 名 `tsgo` 例外）；删掉 divider 注释，用 rustdoc 段落组织 | FAIL |
| **D6** | **依赖序倒置**：解析 `README` 的 phase→包 映射，对每个 `internal/<pkg>` 用 `rg -o "microsoft/typescript-go/internal/[a-z0-9]+"`（仅非测试 `.go`）求内部依赖；依赖被排在更后 phase 即倒置 | phase 必须按真实依赖 DAG 排（叶子先行），倒置 = 后面的包还没移植，前面的包就编译不过 | **调整 `README` 的 phase→包 映射**，把被依赖包前移（内容性决策，脚本不自动改） | FAIL |
| **D7** | 若装了 `markdownlint`/`mdl` 则跑，否则优雅跳过 | markdown 规范化（可选） | 装 `markdownlint-cli` 后按其提示修 | WARN |

### Phase-1 特殊处理

`phase-1-foundation` 可能仍在被其它 subagent 写入。`gate-docs.sh` 把 **phase-1 的所有问题单列**到
「Phase-1（可能尚在生成中）」小节，且**不计入失败**（退出码不受其影响）。phase-1 收口时应回头清掉这些。

### D6 的已知约定

- 只扫**非测试** `.go`（排除 `*_test.go`），代表实现期的真实依赖边。
- 依赖按**顶层包**聚合（`internal/lsp/lsproto` → `lsp`），与 phase 映射的粒度一致。
- 已知非 crate token（如 `testdata` fixture 语料）在 D1b 中跳过。
- 内部依赖若不在任何 phase 映射，列入「未映射依赖（信息）」，不判失败（提示可能漏规划的辅助包）。

---

## B. 代码 gate（`gate-code.sh`）

> 仓库根**无 `Cargo.toml` 时打印「尚无 Rust crate，跳过代码 gate」并退出 0**。
> 一旦 Cargo workspace 落地即自动启用全部检查。无 `cargo` 时工具链类检查（C1-C3/C5）跳过，仍跑静态扫描（C4/C6/C7）。

| ID | 检查什么 | 为何 | 怎么修 |
|---|---|---|---|
| **C1** | `cargo fmt --all --check` | 统一格式 | `cargo fmt --all` |
| **C2** | `cargo clippy --all-targets --all-features -- -D warnings` | warning 即错误 | 按 clippy 提示修 |
| **C3** | `cargo test --all`（含 doctest） | 测试全绿 + doctest 可跑（PORTING §7 的 `# Examples` 要能编译） | 修测试/实现 |
| **C4** | **unsafe gate**：`internal/**/*.rs` 与 `cmd/**/*.rs` 中每个 `unsafe` 须同行或上一行有 `// SAFETY:` | 呼应 PORTING「尽量零 unsafe」；必须的 unsafe 要写清安全前提 | 在 unsafe 上方加 `// SAFETY: <为何安全>`；或改用 arena 索引消除 unsafe |
| **C5** | **rustdoc 公开项 gate**：`RUSTDOCFLAGS="-D missing_docs" cargo doc --no-deps --all` | 每个公开项必须有 rustdoc（PORTING §7 红线） | 给 `pub` 项补 `///`（含 `# Examples` + Side effects） |
| **C6** | **test-go-parity gate**：每个含 `// Go:` 锚的实现 `.rs`，必须有含 `// Go:` 锚的对应测试（同文件 `#[cfg(test)] mod tests` 或 `tests/`） | 镜像 story `audit-test-go-parity.sh`——防「实现声称对齐 Go，但测试没有一条用 Go 实测值兜底」 | 给该文件加测试，每条带 `// Go: <go-file>:<Func>/<case>` |
| **C7** | **stub-readiness gate**：每个 `todo!()`/`unimplemented!()` 须紧邻（同/上/下行）`// DEFER(phase-N)` 且有 `// blocked-by:` 行 | 镜像 story `audit-stub-readiness.sh`——防「标 DEFER 就停」却没验证依赖是否真未 ship | 在 stub 旁补 `// DEFER(phase-N): <原因>` + `// blocked-by: <人话解释>` |

> C6/C7 是 story 复盘的核心教训落地：结构性 gate（有没有 Go 注释 / 有没有 blocked-by）**只能保证形式**，
> 不能保证「测试覆盖了 Go 全部行为」。它们是底线，不是全部——写测试前仍须**先读 Go 源 5 分钟**
> 取 ground truth（见 PORTING §8 与 story testing-discipline.md §2 的 8 大盲区）。

---

## C. Phase 收口纪律（顶层 `[x]` 的判定）

**phase 收口前必须 `gate.sh` 全绿，才能在 `README.md` 进度表把该 phase 打 `[x]`。** 具体：

1. `bash docs/rust-rewrite/scripts/gate.sh --strict` 退出 0（文档 + 代码双绿；该 phase 范围内无 FAIL/WARN）。
2. 该 phase 的 `impl.md` 实现类 TODO 全部 `[x]`，`tests.md` 应收口测试行均 `✓`（推迟标 `—`）。
3. D6 依赖序无倒置（若有，先在 `README` 调整 phase→包 映射，再收口）。

> 与 story 一脉相承：「README 顶层 `[x]` ≠ 真完成」——必须 gate 通过才打勾。

---

## D.（可选）CI 接入示例

> **以下仅为示例，写在本文档供参考；本任务不创建/修改仓库根的真实 CI 配置。**
> 如需启用，把片段放进 `.github/workflows/rust-rewrite-gate.yml`（由仓库 owner 决定）。

```yaml
name: rust-rewrite-gate
on:
  pull_request:
    paths:
      - 'docs/rust-rewrite/**'
      - 'internal/**/*.rs'
      - 'cmd/**/*.rs'
      - 'Cargo.toml'
jobs:
  docs-gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install ripgrep
        run: sudo apt-get update && sudo apt-get install -y ripgrep
      - name: Documentation gate
        run: bash docs/rust-rewrite/scripts/gate-docs.sh
  code-gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Code gate (no-op until Cargo workspace lands)
        run: bash docs/rust-rewrite/scripts/gate-code.sh
```

> 注意：D6 依赖序倒置当前为 RED（见下）——若直接接入 CI，文档 gate 会一直红，直到 phase→包 映射调整完成。
> 可先用 `--docs-only` 跑非 D6 项，或在 owner 决定 phase 重排后再开 D6 强约束。

---

## E. 已知 RED 项（截至本 gate 落地）

`gate-docs.sh` 当前抓到 **12 处跨 phase 依赖序倒置**（D6），这些是**计划级**问题，需人工决策调整
`README` 的 phase→包 映射后才能转绿（脚本不自动重排）：

| 包(当前 phase) | 依赖 | 被依赖包 phase | 非测试导入示例 |
|---|---|---|---|
| `checker`(P4) | `printer` | P5 | `internal/checker/nodebuilder.go` |
| `checker`(P4) | `tracing` | P6 | `internal/checker/tracer.go` |
| `checker`(P4) | `tsoptions` | P6 | `internal/checker/checker.go` |
| `modulespecifiers`(P4) | `outputpaths` | P5 | `internal/modulespecifiers/specifiers.go` |
| `modulespecifiers`(P4) | `tsoptions` | P6 | `internal/modulespecifiers/specifiers.go` |
| `printer`(P5) | `tsoptions` | P6 | `internal/printer/emithost.go` |
| `ls`(P7) | `lsp` | P8 | `internal/ls/*`（`lsp/lsproto` 协议类型） |
| `ls`(P7) | `project` | P8 | `internal/ls/autoimport/registry.go` |
| `ls`(P7) | `bundled` | P9 | `internal/ls/*` |
| `lsp`(P8) | `bundled` | P9 | `internal/lsp/*` |
| `api`(P8) | `bundled` | P9 | `internal/api/server.go` |
| `execute`(P9) | `testutil` | P10 | `internal/execute/tsctests/sys.go` |

> 处理方向由 owner 定夺（前移被依赖包 / 拆分包 / 接受倒置并以接口隔离）。在决策落地前，本 gate 对 D6 保持 RED 是**有意为之**——它就是用来逼出这个计划级冲突的。
