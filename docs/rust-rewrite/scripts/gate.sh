#!/usr/bin/env bash
# gate.sh — typescript-go → Rust 移植「质量 Gate 总入口」
#
# 聚合文档 gate（gate-docs.sh）与代码 gate（gate-code.sh）。任一 fail 则非零退出，
# 末尾打印打分项小结。phase 收口前必须本脚本全绿，方可在 README 进度打 [x]。
#
# 用法：
#   bash docs/rust-rewrite/scripts/gate.sh               # 文档 + 代码 gate
#   bash docs/rust-rewrite/scripts/gate.sh --docs-only   # 只跑文档 gate
#   bash docs/rust-rewrite/scripts/gate.sh --code-only   # 只跑代码 gate
#   bash docs/rust-rewrite/scripts/gate.sh --strict      # 文档 gate 的 WARN 也计入失败
#
# 退出码： 0 = 全绿； 1 = 至少一个子 gate 失败

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RUN_DOCS=1
RUN_CODE=1
STRICT=0
for arg in "$@"; do
	case "$arg" in
		--docs-only) RUN_CODE=0 ;;
		--code-only) RUN_DOCS=0 ;;
		--strict)    STRICT=1 ;;
		-h|--help)
			grep -E '^#( |$)' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
			exit 0 ;;
		*)
			echo "未知参数：$arg（可用：--docs-only | --code-only | --strict）" >&2
			exit 2 ;;
	esac
done

DOCS_RESULT="跳过"
CODE_RESULT="跳过"
DOCS_RC=0
CODE_RC=0

if [ "$RUN_DOCS" = "1" ]; then
	echo "################################################################"
	echo "# 文档 Gate"
	echo "################################################################"
	docs_args=()
	[ "$STRICT" = "1" ] && docs_args+=("--strict")
	if bash "$SCRIPT_DIR/gate-docs.sh" ${docs_args[@]+"${docs_args[@]}"}; then
		DOCS_RESULT="GREEN"
	else
		DOCS_RC=1
		DOCS_RESULT="RED"
	fi
	echo ""
fi

if [ "$RUN_CODE" = "1" ]; then
	echo "################################################################"
	echo "# 代码 Gate"
	echo "################################################################"
	if bash "$SCRIPT_DIR/gate-code.sh"; then
		CODE_RESULT="GREEN"
	else
		CODE_RC=1
		CODE_RESULT="RED"
	fi
	echo ""
fi

# ─── 总分小结 ────────────────────────────────────────────────────────────────
echo "================================================================"
echo " 质量 Gate 总小结"
echo "----------------------------------------------------------------"
printf "  %-12s %s\n" "文档 Gate:" "$DOCS_RESULT"
printf "  %-12s %s\n" "代码 Gate:" "$CODE_RESULT"
echo "================================================================"

if [ "$DOCS_RC" -ne 0 ] || [ "$CODE_RC" -ne 0 ]; then
	echo "结果：RED — 存在未通过的 gate，禁止在 README 进度打 [x]。"
	exit 1
fi
echo "结果：GREEN — 全部 gate 通过。"
exit 0
