#!/usr/bin/env bash
# gate-docs.sh — typescript-go → Rust 移植「文档质量 Gate」
#
# 纯 bash + ripgrep，不依赖 Rust 代码是否存在。对 docs/rust-rewrite/ 下的移植
# 文档做只读检查，把「文档纪律」从口头约定硬化为可运行 / 可 CI 接入的门禁。
#
# 检查项（详见 docs/rust-rewrite/references/gates.md）：
#   D1 结构完整     —— 每个 phase-*/<pkg>/ 同时有 impl.md + tests.md；每个 phase-* 有 README.md
#   D1b 计划落地     —— README 的 phase→包 映射里每个包都有对应文档目录（缺失=WARN）
#   D2 Go 锚点      —— 每个 impl.md/tests.md 至少 1 个 `// Go:` 锚；tests.md 应有 "Go 对照" 列（缺=WARN）
#   D3 checkbox 纪律 —— impl.md「实现 TODO」段只能用 - [ ] / - [x]，不许裸 bullet
#   D4 完成列图例    —— tests.md 含 ✓ / — 图例
#   D5 命名红线      —— Cargo crate 名须 tsgo_ 前缀；禁止 section divider 注释（// ==== / /* === / // ----）
#   D6 依赖序 gate   —— 解析 README phase→包 映射，抓「包 A(Px) 依赖 包 B(Py>x)」的跨 phase 倒置
#   D7 markdownlint  —— 若系统装了 markdownlint / mdl 则跑，否则优雅跳过
#
# 用法：
#   bash docs/rust-rewrite/scripts/gate-docs.sh            # 检查并报告
#   bash docs/rust-rewrite/scripts/gate-docs.sh --strict   # WARN 也计入失败
#
# 退出码：
#   0 — 无 FAIL（--strict 下亦无 WARN）
#   1 — 存在 FAIL（含依赖序倒置）
#
# 说明：phase-1-foundation 可能仍在被其它 subagent 写入。本脚本把 phase-1 的所有
# 问题单列到「Phase-1（可能尚在生成中）」小节，且**不计入失败**。

set -euo pipefail

# ─── 定位仓库根与文档目录（脚本自身定位，回退 git） ─────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"          # docs/rust-rewrite
REPO_ROOT="$(cd "$DOCS_DIR/../.." && pwd)"        # 仓库根
if [ ! -d "$REPO_ROOT/internal" ] && command -v git >/dev/null 2>&1; then
	REPO_ROOT="$(git -C "$DOCS_DIR" rev-parse --show-toplevel 2>/dev/null || echo "$REPO_ROOT")"
fi

STRICT=0
if [ "${1:-}" = "--strict" ]; then STRICT=1; fi

# ─── 收集桶 ──────────────────────────────────────────────────────────────────
FAILS=()           # 非 P1 致命问题
WARNS=()           # 非 P1 告警
P1NOTES=()         # phase-1 问题（不计入失败）
DEP_VIOLATIONS=()  # 构建边依赖序倒置（致命）
DEP_UNMAPPED=()    # 内部依赖未出现在任何 phase 映射（信息）
DEP_TEST_EDGES=()  # 仅测试边（dev-dep）：看似倒置但只在 *_test.go（信息）
DEP_TESTINFRA=()   # 指向测试设施包的边（dev-dep）（信息）

# 统计
N_PHASES=0
N_PKGS=0
N_IMPL=0
N_TESTS=0

# record_issue <FAIL|WARN> <is_p1:0|1> <message>
record_issue() {
	local sev="$1" p1="$2" msg="$3"
	if [ "$p1" = "1" ]; then
		P1NOTES+=("[$sev] $msg")
	elif [ "$sev" = "FAIL" ]; then
		FAILS+=("$msg")
	else
		WARNS+=("$msg")
	fi
}

# 相对仓库根的短路径，便于阅读
relpath() { echo "${1#"$REPO_ROOT"/}"; }

# ─── 解析 README phase→包 映射 → 临时文件 "pkg<空格>phaseNum" ────────────────
README="$DOCS_DIR/README.md"
PHASEMAP="$(mktemp)"      # 行： <pkg-basename> <phaseNum>
PHASEDIRS="$(mktemp)"     # 行： <phaseNum> <phase-dir-name>
trap 'rm -f "$PHASEMAP" "$PHASEDIRS"' EXIT

# phaseNum → phase 目录名
for d in "$DOCS_DIR"/phase-*; do
	[ -d "$d" ] || continue
	base="$(basename "$d")"
	num="$(echo "$base" | sed -nE 's/^phase-([0-9]+)-.*/\1/p')"
	[ -n "$num" ] && echo "$num $base" >> "$PHASEDIRS"
done

if [ -f "$README" ]; then
	# 进度行形如： - [ ] **P1 地基** — `stringutil` `json` ...
	while IFS= read -r line; do
		phase="$(echo "$line" | grep -oE '\*\*P[0-9]+' | grep -oE '[0-9]+' | head -1 || true)"
		[ -z "$phase" ] && continue
		# 抽出所有 `...` 反引号 token；token 即 crate key（保留全名，含 ls/lsconv、cmd/tsgo 等）
		echo "$line" | grep -oE '`[^`]+`' | tr -d '`' | while IFS= read -r tok; do
			[ -z "$tok" ] && continue
			echo "$tok $phase"
		done
	done < <(grep -E '^- \[.\] \*\*P[0-9]+' "$README" || true) >> "$PHASEMAP"
else
	record_issue FAIL 0 "README 缺失：$(relpath "$README")（无法解析 phase→包 映射，依赖序 gate 跳过）"
fi

# crate key → phaseNum 查询
phase_of() { awk -v k="$1" '$1==k{print $2; exit}' "$PHASEMAP"; }
# phaseNum → 目录名
dir_of_phase() { awk -v k="$1" '$1==k{print $2; exit}' "$PHASEDIRS"; }

# ─── 子包粒度的 crate 边界（拆分子 crate）与测试设施判定 ──────────────────────
# 拆分子 crate（破环 / 解倒置）。其余子包默认归并到顶层包 crate。
# 见 references/crate-map.md 与 references/gates.md §D6。
SUBCRATES="lsproto ls/lsconv ls/lsutil ls/change ls/autoimport project/dirty project/logging"

# 目录（如 internal/lsp/lsproto 或 cmd/tsgo）→ crate key
crate_of() {
	local d="$1" rel
	case "$d" in cmd/*) echo "cmd/tsgo"; return ;; esac
	rel="${d#internal/}"
	case "$rel" in
		lsp/lsproto) echo "lsproto"; return ;;
		ls/lsconv) echo "ls/lsconv"; return ;;
		ls/lsutil) echo "ls/lsutil"; return ;;
		ls/change) echo "ls/change"; return ;;
		ls/autoimport) echo "ls/autoimport"; return ;;
		project/dirty) echo "project/dirty"; return ;;
		project/logging) echo "project/logging"; return ;;
	esac
	echo "${rel%%/*}"
}

# 测试设施 / 测试帮助包目录（不约束生产拓扑：作为源整体豁免，作为目标按 dev-dep）
is_testinfra_dir() {
	local d="$1" rel base
	rel="${d#internal/}"
	case "$rel" in
		testutil|testutil/*|testrunner|testrunner/*|fourslash|fourslash/*) return 0 ;;
	esac
	base="${d##*/}"
	case "$base" in *test|*tests|*testutil|*mock) return 0 ;; esac
	return 1
}

# 某目录（绝对路径）的内部 import（注释行过滤；mode=build|test）→ 目录路径列表
imports_of() {
	local d="$1" mode="$2" g
	if [ "$mode" = "build" ]; then g='!*_test.go'; else g='*_test.go'; fi
	rg --no-filename --max-depth 1 -g '*.go' -g "$g" \
		'microsoft/typescript-go/(internal|cmd)/[a-zA-Z0-9_/]+' "$d" 2>/dev/null \
		| grep -vE '^[[:space:]]*(//|/\*|\*)' \
		| grep -oE 'microsoft/typescript-go/(internal|cmd)/[a-zA-Z0-9_/]+' \
		| sed 's#github.com/##; s#microsoft/typescript-go/##' | sort -u || true
}

# ─── 文件级内容检查 helper ───────────────────────────────────────────────────
# 是否含 `// Go:` 锚
check_go_anchor() {
	local f="$1" is_p1="$2"
	if ! grep -q "// Go:" "$f"; then
		record_issue FAIL "$is_p1" "缺 // Go: 锚点：$(relpath "$f")"
	fi
}

# impl.md「实现 TODO」段裸 bullet 检查
check_checkbox_discipline() {
	local f="$1" is_p1="$2"
	local bad
	bad="$(awk '
		/^## / {
			if ($0 ~ /实现/ && $0 ~ /TODO/) { inseg=1 } else { inseg=0 }
			next
		}
		inseg && /^- / && $0 !~ /^- \[[ xX]\] / { print NR": "$0 }
	' "$f" || true)"
	if [ -n "$bad" ]; then
		local first
		first="$(echo "$bad" | head -1)"
		record_issue FAIL "$is_p1" "impl.md「实现 TODO」段有裸 bullet（须 - [ ]/- [x]）：$(relpath "$f") 行 $first"
	fi
}

# tests.md 完成列图例 ✓ / —
check_legend() {
	local f="$1" is_p1="$2"
	if ! grep -q "✓" "$f" || ! grep -q "—" "$f"; then
		record_issue FAIL "$is_p1" "tests.md 缺完成列图例（须含 ✓ 与 —）：$(relpath "$f")"
	fi
}

# tests.md 参照列：接受 "Go 对照"（主表）或 "依据"（0 直接单测补充表）（缺 → WARN）
check_go_column() {
	local f="$1" is_p1="$2"
	if ! grep -q "Go 对照" "$f" && ! grep -q "依据" "$f"; then
		record_issue WARN "$is_p1" "tests.md 表缺 'Go 对照'/'依据' 参照列：$(relpath "$f")"
	fi
}

# 命名红线：Cargo crate 名须 tsgo 前缀（tsgo_<pkg> 库；cmd/tsgo bin 名为 tsgo）
# 仅匹配「行首（可缩进）name =」形式，避免误伤 fileName=/thread_name=/测试数据 name=...
check_crate_naming() {
	local f="$1" is_p1="$2"
	local bad
	bad="$(grep -nE '^[[:space:]]*name[[:space:]]*=[[:space:]]*"[^"]+"' "$f" 2>/dev/null \
		| grep -vE 'name[[:space:]]*=[[:space:]]*"tsgo' || true)"
	if [ -n "$bad" ]; then
		record_issue FAIL "$is_p1" "crate 名未用 tsgo 前缀：$(relpath "$f") → $(echo "$bad" | head -1)"
	fi
}

# 禁止 section divider 注释（仅在 ``` 代码围栏内检查，避免误伤描述 baseline 输出格式的散文/表格）
check_no_divider() {
	local f="$1" is_p1="$2"
	local bad
	bad="$(awk '
		/^[[:space:]]*```/ { infence = !infence; next }
		infence && (/\/\/[[:space:]]*={3,}/ || /\/\*[[:space:]]*={3,}/ || /\/\/[[:space:]]*-{4,}/) { print NR": "$0 }
	' "$f" || true)"
	if [ -n "$bad" ]; then
		record_issue FAIL "$is_p1" "代码围栏内出现 section divider 注释（禁止）：$(relpath "$f") → $(echo "$bad" | head -1)"
	fi
}

# ─── D1 / D1b / D2-D5：遍历 phase 目录 ───────────────────────────────────────
for phase_dir in "$DOCS_DIR"/phase-*; do
	[ -d "$phase_dir" ] || continue
	N_PHASES=$((N_PHASES + 1))
	pbase="$(basename "$phase_dir")"
	is_p1=0
	case "$pbase" in phase-1-*) is_p1=1 ;; esac

	# D1: 每个 phase 有 README.md
	if [ ! -f "$phase_dir/README.md" ]; then
		record_issue FAIL "$is_p1" "phase 缺 README.md：$(relpath "$phase_dir")/README.md"
	fi

	# D1 + D2-D5: 每个包子目录
	for pkg_dir in "$phase_dir"/*/; do
		[ -d "$pkg_dir" ] || continue
		N_PKGS=$((N_PKGS + 1))
		impl="${pkg_dir}impl.md"
		tests="${pkg_dir}tests.md"

		if [ -f "$impl" ]; then
			N_IMPL=$((N_IMPL + 1))
			check_go_anchor "$impl" "$is_p1"
			check_checkbox_discipline "$impl" "$is_p1"
			check_crate_naming "$impl" "$is_p1"
			check_no_divider "$impl" "$is_p1"
		else
			record_issue FAIL "$is_p1" "包缺 impl.md：$(relpath "$pkg_dir")impl.md"
		fi

		if [ -f "$tests" ]; then
			N_TESTS=$((N_TESTS + 1))
			check_go_anchor "$tests" "$is_p1"
			check_legend "$tests" "$is_p1"
			check_go_column "$tests" "$is_p1"
			check_crate_naming "$tests" "$is_p1"
			check_no_divider "$tests" "$is_p1"
		else
			record_issue FAIL "$is_p1" "包缺 tests.md：$(relpath "$pkg_dir")tests.md"
		fi
	done
done

# D1b: README 映射里每个包都应有文档目录（缺=WARN / P1 单列）
# 例外：fixture 语料（testdata）与「文档随父包」的拆分子 crate（其移植细节写在父包 impl.md）。
if [ -f "$README" ] && [ -s "$PHASEMAP" ]; then
	while read -r pkg phase; do
		[ -z "$pkg" ] && continue
		case "$pkg" in
			testdata|lsproto|ls/lsconv|ls/lsutil|ls/change|ls/autoimport|project/dirty|project/logging) continue ;;
			cmd/tsgo) cand="cmd-tsgo" ;;
			*) cand="$pkg" ;;
		esac
		pdir="$(dir_of_phase "$phase")"
		[ -z "$pdir" ] && continue
		p1flag=0
		case "$pdir" in phase-1-*) p1flag=1 ;; esac
		if [ ! -d "$DOCS_DIR/$pdir/$cand" ]; then
			record_issue WARN "$p1flag" "README 列出的包无文档目录：P$phase \`$pkg\`（期望 $pdir/$cand/）"
		fi
	done < "$PHASEMAP"
fi

# ─── D6：依赖序 gate（核心，子包粒度 + 边分类） ───────────────────────────────
# 枚举每个 Go 包目录（含子包），抽其内部 import：
#   构建边（非 *_test.go）：必须被 phase 序尊重（合法拓扑序）→ 倒置即致命。
#   仅测试边（只在 *_test.go）：dev-dep，不约束 phase，看似倒置者列为信息。
#   测试设施包（testutil/testrunner/fourslash、*tests/*testutil/*mock 子包）：
#     作为源整体豁免；作为目标按 dev-dep。
# 目录按 crate_of() 归并到 crate；crate→phase 取自 README 映射。
if [ -s "$PHASEMAP" ] && [ -d "$REPO_ROOT/internal" ]; then
	D6BUILD="$(mktemp)"; D6TEST="$(mktemp)"
	D6INFRA="$(mktemp)"; D6UNMAP="$(mktemp)"
	pkgdirs="$(cd "$REPO_ROOT" && rg --files -g '*.go' internal cmd/tsgo 2>/dev/null \
		| sed 's#/[^/]*$##' | sort -u || true)"
	while IFS= read -r dir; do
		[ -z "$dir" ] && continue
		is_testinfra_dir "$dir" && continue          # 测试设施源整体豁免
		scr="$(crate_of "$dir")"
		# 构建边
		while IFS= read -r imp; do
			[ -z "$imp" ] && continue
			[ "$imp" = "$dir" ] && continue
			dcr="$(crate_of "$imp")"
			[ "$dcr" = "$scr" ] && continue
			if is_testinfra_dir "$imp"; then echo "$scr|$dcr" >> "$D6INFRA"; continue; fi
			echo "$scr|$dcr" >> "$D6BUILD"
		done < <(imports_of "$REPO_ROOT/$dir" build)
		# 仅测试边（先全部记下，后续剔除构建边）
		while IFS= read -r imp; do
			[ -z "$imp" ] && continue
			[ "$imp" = "$dir" ] && continue
			dcr="$(crate_of "$imp")"
			[ "$dcr" = "$scr" ] && continue
			is_testinfra_dir "$imp" && continue
			echo "$scr|$dcr" >> "$D6TEST"
		done < <(imports_of "$REPO_ROOT/$dir" test)
	done <<< "$pkgdirs"

	# 构建边：判倒置（pb>pa 致命；pb==pa 同 phase 合法）
	while IFS='|' read -r A B; do
		[ -z "$A" ] && continue
		pa="$(phase_of "$A")"; pb="$(phase_of "$B")"
		if [ -z "$pa" ] || [ -z "$pb" ]; then
			echo "$A -> $B" >> "$D6UNMAP"; continue
		fi
		if [ "$pb" -gt "$pa" ]; then
			DEP_VIOLATIONS+=("$A(P$pa) → 依赖 $B(P$pb>P$pa)")
		fi
	done < <(sort -u "$D6BUILD")

	# 仅测试边（剔除已是构建边的）：只列「若按构建边会倒置」的，作为信息
	while IFS='|' read -r A B; do
		[ -z "$A" ] && continue
		pa="$(phase_of "$A")"; pb="$(phase_of "$B")"
		[ -z "$pa" ] && continue; [ -z "$pb" ] && continue
		if [ "$pb" -gt "$pa" ]; then
			DEP_TEST_EDGES+=("$A(P$pa) ⇢ $B(P$pb)〔仅测试〕")
		fi
	done < <(comm -23 <(sort -u "$D6TEST") <(sort -u "$D6BUILD"))

	# 指向测试设施的边（dev-dep 信息）
	while IFS='|' read -r A B; do
		[ -z "$A" ] && continue
		DEP_TESTINFRA+=("$A ⇢ $B〔测试设施〕")
	done < <(sort -u "$D6INFRA")

	# 未映射构建边（信息）
	while IFS= read -r line; do
		[ -z "$line" ] && continue
		DEP_UNMAPPED+=("$line")
	done < <(sort -u "$D6UNMAP")

	rm -f "$D6BUILD" "$D6TEST" "$D6INFRA" "$D6UNMAP"
fi

# ─── D7：markdownlint / mdl（可选） ───────────────────────────────────────────
MDLINT_RESULT="跳过（系统未安装 markdownlint / mdl）"
if command -v markdownlint >/dev/null 2>&1; then
	if markdownlint "$DOCS_DIR"/**/*.md >/tmp/gate-docs-mdlint.log 2>&1; then
		MDLINT_RESULT="markdownlint 通过"
	else
		MDLINT_RESULT="markdownlint 有告警（见 /tmp/gate-docs-mdlint.log）"
		record_issue WARN 0 "markdownlint 报告问题（见 /tmp/gate-docs-mdlint.log）"
	fi
elif command -v mdl >/dev/null 2>&1; then
	if mdl "$DOCS_DIR" >/tmp/gate-docs-mdl.log 2>&1; then
		MDLINT_RESULT="mdl 通过"
	else
		MDLINT_RESULT="mdl 有告警（见 /tmp/gate-docs-mdl.log）"
		record_issue WARN 0 "mdl 报告问题（见 /tmp/gate-docs-mdl.log）"
	fi
fi

# ─── 报告 ────────────────────────────────────────────────────────────────────
echo "================================================================"
echo " 文档 Gate（gate-docs.sh）"
echo " 仓库根：$REPO_ROOT"
echo " 文档根：$(relpath "$DOCS_DIR")"
echo "================================================================"
echo ""
echo "扫描：$N_PHASES 个 phase / $N_PKGS 个包目录 / impl.md $N_IMPL · tests.md $N_TESTS"
echo ""

print_bucket() {
	local title="$1"; shift
	local n=$#
	if [ "$n" -gt 0 ]; then
		echo "${title}（${n}）："
		for item in "$@"; do echo "  - $item"; done
		echo ""
	fi
}

# 依赖序 violation（核心，单独高亮）
if [ ${#DEP_VIOLATIONS[@]} -gt 0 ]; then
	echo "【D6 构建边依赖序倒置 — 致命】（包 A 依赖排在更后 phase 的包 B）："
	printf '%s\n' "${DEP_VIOLATIONS[@]}" | sort -u | while IFS= read -r v; do echo "  ✗ $v"; done
	echo "  → 修法：调整 README 的 phase→包 映射，把被依赖包前移 / 必要时拆出子 crate。"
	echo ""
else
	echo "【D6 构建边依赖序】GREEN — 所有构建边都尊重 phase 拓扑序（同 phase 内允许互依）。"
	echo ""
fi

if [ ${#DEP_TEST_EDGES[@]} -gt 0 ]; then
	echo "【D6 信息·仅测试边（dev-dep，不约束 phase 序）】看似倒置但只在 *_test.go："
	printf '%s\n' "${DEP_TEST_EDGES[@]}" | sort -u | while IFS= read -r u; do echo "  · $u"; done
	echo ""
fi

if [ ${#DEP_TESTINFRA[@]} -gt 0 ]; then
	echo "【D6 信息·测试设施边（dev-dep）】指向 testutil/testrunner/fourslash 等："
	printf '%s\n' "${DEP_TESTINFRA[@]}" | sort -u | while IFS= read -r u; do echo "  · $u"; done
	echo ""
fi

if [ ${#DEP_UNMAPPED[@]} -gt 0 ]; then
	echo "【D6 信息】内部构建依赖未出现在任何 phase 映射（可能是未规划的辅助包）："
	printf '%s\n' "${DEP_UNMAPPED[@]}" | sort -u | while IFS= read -r u; do echo "  · $u"; done
	echo ""
fi

if [ ${#FAILS[@]} -gt 0 ]; then
	print_bucket "【FAIL】文档纪律违规" "${FAILS[@]}"
fi
if [ ${#WARNS[@]} -gt 0 ]; then
	print_bucket "【WARN】建议修复" "${WARNS[@]}"
fi
if [ ${#P1NOTES[@]} -gt 0 ]; then
	echo "【Phase-1（可能尚在生成中，不计入失败）】（${#P1NOTES[@]}）："
	for item in "${P1NOTES[@]}"; do echo "  · $item"; done
	echo ""
fi

echo "markdownlint：$MDLINT_RESULT"
echo ""

# ─── 小结 + 退出码 ───────────────────────────────────────────────────────────
N_FAIL=${#FAILS[@]}
N_WARN=${#WARNS[@]}
N_DEP=${#DEP_VIOLATIONS[@]}
N_P1=${#P1NOTES[@]}
TOTAL_FATAL=$((N_FAIL + N_DEP))
if [ "$STRICT" = "1" ]; then
	TOTAL_FATAL=$((TOTAL_FATAL + N_WARN))
fi

echo "----------------------------------------------------------------"
echo " 小结： FAIL=$N_FAIL  依赖倒置=$N_DEP  WARN=$N_WARN  P1(不计)=$N_P1"
echo "----------------------------------------------------------------"

if [ "$TOTAL_FATAL" -gt 0 ]; then
	echo "结果：RED（$TOTAL_FATAL 项致命）。phase 收口前必须清零方可在 README 进度打 [x]。"
	exit 1
fi
echo "结果：GREEN（非 P1 范围内文档纪律通过）。"
exit 0
