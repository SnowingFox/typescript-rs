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
| **D6** | **依赖序倒置（子包粒度 + 边分类）**：枚举每个 Go 包目录（含子包），抽其内部 import（注释行过滤）；按 `crate_of()` 归并到 crate，crate→phase 取自 `README` 映射。**构建边**（非 `*_test.go`）若指向更后 phase 的 crate 即倒置（同 phase 内互依合法）。**仅测试边** / **测试设施边**列为信息，不约束 | phase 必须按真实**构建**依赖 DAG 排（叶子先行），倒置 = 后面的包还没移植，前面的包就编译不过；测试边是 dev-dep 不影响构建序 | **调整 `README` 的 phase→包 映射** / 必要时**拆出子 crate**（如 `lsproto`/`ls/*`/`project/{dirty,logging}`） | FAIL |
| **D7** | 若装了 `markdownlint`/`mdl` 则跑，否则优雅跳过 | markdown 规范化（可选） | 装 `markdownlint-cli` 后按其提示修 | WARN |

### Phase-1 特殊处理

`phase-1-foundation` 可能仍在被其它 subagent 写入。`gate-docs.sh` 把 **phase-1 的所有问题单列**到
「Phase-1（可能尚在生成中）」小节，且**不计入失败**（退出码不受其影响）。phase-1 收口时应回头清掉这些。

### D6 的边分类与约定（重要）

D6 把内部 import 边分三类，**只有「构建边」约束 phase 拓扑序**：

| 边类型 | 判定 | 处理 |
|---|---|---|
| **构建边** | 出现在非 `*_test.go` | 必须合法拓扑：依赖的 crate 不得在更后 phase（同 phase 合法）。倒置 = FAIL |
| **仅测试边** | 只在 `*_test.go` | → Rust `[dev-dependencies]`，不约束 phase；「看似倒置」者列为信息（如 `checker→compiler`、`ls/autoimport→project`、`printer→transformers`、`tsoptions→diagnosticwriter`） |
| **测试设施边** | 源或目标是 `testutil`/`testrunner`/`fourslash` 或 `*tests`/`*testutil`/`*mock` 子包 | 源整体豁免；目标按 dev-dep，列为信息 |

其它约定：

- **注释行过滤**：抽 import 时排除以 `//`/`/*`/`*` 开头的行（避免把注释掉的 import 误判为依赖——曾导致 `ls/autoimport→ls` 假环）。
- **子包粒度**：节点 = 每个 Go 包目录；`crate_of()` 把目录归并到 crate——默认顶层包；7 个**拆分子 crate**（`lsp/lsproto`、`ls/{lsconv,lsutil,change,autoimport}`、`project/{dirty,logging}`）各自独立。其余子目录归并父 crate。
- **README token 即 crate key**：`lsproto`、`ls/lsconv`、`cmd/tsgo` 等全名 token 直接作 key（不取 basename）。
- **D1b 例外**：`testdata`（fixture 语料）与 7 个拆分子 crate（文档随父包 impl.md）在 D1b 中跳过「无文档目录」告警。
- 未映射构建依赖列为信息（提示可能漏规划的辅助包）。

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

> 提示：D6 现已 GREEN（见 §E）。CI 可直接接入 `gate-docs.sh`（全绿退出 0）。

---

## E. 依赖序倒置：已通过重排 + 拆分解决（D6 GREEN）

初版 gate 抓到 **12 处跨 phase 构建倒置**。本轮按"构建边/仅测试边/测试设施边"三类口径重排，已全部解决，`gate-docs.sh` **D6 GREEN（退出 0）**：

**12 处的归宿：**

| 原倒置 | 归类 / 处理 |
|---|---|
| `checker(P4)→printer(P5)` | `printer` 前移 P4 |
| `checker(P4)→tracing(P6)` | `tracing` 前移 P4 |
| `checker(P4)→tsoptions(P6)` | `tsoptions` 前移 P4 |
| `modulespecifiers(P4)→outputpaths(P5)` | `outputpaths` 前移 P4 |
| `modulespecifiers(P4)→tsoptions(P6)` | `tsoptions` 前移 P4 |
| `printer(P5)→tsoptions(P6)` | 二者均 P4 |
| `ls(P7)→lsp(P8)` | 实为 `ls/*→lsp/lsproto`；`lsproto` 拆 crate 前移 P7（`ls→lsp` 主体仅在 `*_test.go`，dev-dep） |
| `ls(P7)→project(P8)` | 实为 `ls/autoimport→project/{dirty,logging}`；后两者拆 crate 前移 P1（`ls→project` 主体仅测试，dev-dep） |
| `ls(P7)→bundled(P9)` | `bundled` 前移 P1 |
| `lsp(P8)→bundled(P9)` | `bundled` 前移 P1 |
| `api(P8)→bundled(P9)` | `bundled` 前移 P1 |
| `execute(P9)→testutil(P10)` | 实为 `execute/tsctests→testutil`，`tsctests` 是测试设施子包 → dev-dep 豁免（已核实生产 execute 非测试代码不依赖 testutil） |

**最终合法 phase 次序**（gate D6 据 `README` 映射判定为合法拓扑序）：

- **P1** + `jsonrpc` `bundled` `project/dirty` `project/logging`
- **P4** + `outputpaths` `sourcemap` `tracing` `tsoptions` `printer`（checker 的构建前置）
- **P5** = `transformers` `diagnosticwriter`（checker 之后）
- **P6** = `compiler`
- **P7** = `lsproto` `ls/lsconv` `ls/lsutil` `ls/change` `ls/autoimport` `format` `ls`
- **P8** = `project` `api` `lsp`；**P9** = `execute` `cmd/tsgo`

详见根 [README.md](../README.md) 的「依赖序口径」与 [crate-map.md](./crate-map.md) 的拆分/前移/ dev-dep 三表。

> 仍为 dev-dep（仅测试边，gate 列为信息、不阻断）：`checker→compiler`、`ls/autoimport→project`、`printer→transformers`、`tsoptions→diagnosticwriter` 等——落地时声明为 Rust `[dev-dependencies]`。
