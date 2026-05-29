#!/usr/bin/env bash
# gate-code.sh — typescript-go → Rust 移植「代码质量 Gate」
#
# 为 Rust 落地准备的门禁。当前仓库根尚无 Cargo.toml 时优雅 no-op（退出 0）。
# 一旦 Cargo workspace 落地，本脚本即成为每个包收口前必须全绿的硬 gate。
#
# 检查项（详见 docs/rust-rewrite/references/gates.md）：
#   C1 fmt          —— cargo fmt --all --check
#   C2 clippy       —— cargo clippy --all-targets --all-features -- -D warnings
#   C3 test         —— cargo test --all（含 doctest）
#   C4 unsafe gate  —— internal/**/*.rs 与 cmd/**/*.rs 中每个 unsafe 须紧邻 // SAFETY:
#   C5 rustdoc gate —— RUSTDOCFLAGS="-D missing_docs" cargo doc --no-deps --all
#   C6 test-go-parity—— 每个含 // Go: 锚的实现 .rs 必须有含 // Go: 锚的对应测试
#   C7 stub-readiness—— 每个 todo!()/unimplemented!() 须紧邻 // DEFER(phase-N) + // blocked-by:
#   C8 comments-en  —— .rs 代码注释一律英文（注释中禁出现 CJK 字符）
#
# 用法：
#   bash docs/rust-rewrite/scripts/gate-code.sh            # 跑全部
#   bash docs/rust-rewrite/scripts/gate-code.sh --strict   # 预留：当前与默认一致
#
# 退出码： 0 = 无 Rust crate（跳过）或全绿； 1 = 有 gate 失败

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$DOCS_DIR/../.." && pwd)"
if [ ! -d "$REPO_ROOT/internal" ] && command -v git >/dev/null 2>&1; then
	REPO_ROOT="$(git -C "$DOCS_DIR" rev-parse --show-toplevel 2>/dev/null || echo "$REPO_ROOT")"
fi

cd "$REPO_ROOT"

# ─── 无 Rust crate → 优雅 no-op ───────────────────────────────────────────────
if [ ! -f "$REPO_ROOT/Cargo.toml" ]; then
	echo "尚无 Rust crate（仓库根无 Cargo.toml），跳过代码 gate。"
	echo "（一旦 Cargo workspace 落地，本脚本将启用 C1-C7 全部检查。）"
	exit 0
fi

FAILS=()
note_fail() { FAILS+=("$1"); }

echo "================================================================"
echo " 代码 Gate（gate-code.sh） — 仓库根：$REPO_ROOT"
echo "================================================================"

have_cargo=1
if ! command -v cargo >/dev/null 2>&1; then
	have_cargo=0
	echo "WARN：未找到 cargo，C1-C3/C5 工具链类检查跳过；仍跑 C4/C6/C7 静态扫描。"
fi

# ─── C1 fmt ──────────────────────────────────────────────────────────────────
if [ "$have_cargo" = "1" ]; then
	echo ""; echo "[C1] cargo fmt --all --check"
	if cargo fmt --all --check; then
		echo "  C1 OK"
	else
		note_fail "C1 fmt：存在未格式化代码（运行 cargo fmt --all 修复）"
	fi

	# ─── C2 clippy ───────────────────────────────────────────────────────────
	echo ""; echo "[C2] cargo clippy --all-targets --all-features -- -D warnings"
	if cargo clippy --all-targets --all-features -- -D warnings; then
		echo "  C2 OK"
	else
		note_fail "C2 clippy：存在 warning（-D warnings 视为错误）"
	fi

	# ─── C3 test（含 doctest） ─────────────────────────────────────────────────
	echo ""; echo "[C3] cargo test --all"
	if cargo test --all; then
		echo "  C3 OK"
	else
		note_fail "C3 test：测试未全绿（含 doctest）"
	fi

	# ─── C5 rustdoc missing_docs ──────────────────────────────────────────────
	echo ""; echo "[C5] RUSTDOCFLAGS=\"-D missing_docs\" cargo doc --no-deps --all"
	if RUSTDOCFLAGS="-D missing_docs" cargo doc --no-deps --all; then
		echo "  C5 OK"
	else
		note_fail "C5 rustdoc：存在缺失 rustdoc 的公开项（missing_docs）"
	fi
fi

# ─── 待扫描的 Rust 源文件（实现文件，排除 target/） ──────────────────────────
collect_rs() {
	find "$REPO_ROOT/internal" "$REPO_ROOT/cmd" -type f -name '*.rs' 2>/dev/null \
		| grep -v "/target/" || true
}

# ─── C4 unsafe gate ──────────────────────────────────────────────────────────
echo ""; echo "[C4] unsafe 块须紧邻 // SAFETY: 注释"
c4_bad=0
while IFS= read -r f; do
	[ -z "$f" ] && continue
	# 命中代码中的 unsafe token（排除注释/文档行本身提到 unsafe）
	while IFS= read -r m; do
		[ -z "$m" ] && continue
		ln="${m%%:*}"
		# 同行或上一行须含 // SAFETY:
		same="$(sed -n "${ln}p" "$f" 2>/dev/null || true)"
		prev_ln=$((ln - 1))
		prev="$(sed -n "${prev_ln}p" "$f" 2>/dev/null || true)"
		if echo "$same" | grep -q "// SAFETY:" || echo "$prev" | grep -q "// SAFETY:"; then
			:
		else
			echo "  ✗ ${f#"$REPO_ROOT"/}:${ln} unsafe 缺紧邻 // SAFETY:"
			c4_bad=$((c4_bad + 1))
		fi
	done < <(grep -nE '\bunsafe\b' "$f" 2>/dev/null \
		| grep -vE ':[[:space:]]*//' || true)
done < <(collect_rs)
if [ "$c4_bad" -gt 0 ]; then
	note_fail "C4 unsafe：$c4_bad 处 unsafe 缺紧邻 // SAFETY: 注释"
else
	echo "  C4 OK（零 unsafe 或全部已标注 SAFETY）"
fi

# ─── C6 test-go-parity ───────────────────────────────────────────────────────
# 测试约定（PORTING §2/§8）：单测独立成兄弟文件 <stem>_test.rs（镜像 Go _test.go）。
# 故每个含 // Go: 锚的实现文件 <stem>.rs，须有同目录 <stem>_test.rs 且其含 // Go: 锚；
# 同时兼容旧式内联 #[cfg(test)] 锚 与 crate tests/ 目录测试。
echo ""; echo "[C6] 含 // Go: 锚的实现文件须有含 // Go: 锚的对应测试（优先独立 <stem>_test.rs）"
c6_bad=0
while IFS= read -r f; do
	[ -z "$f" ] && continue
	base="$(basename "$f")"
	# 跳过测试文件本身（独立 _test.rs 与 tests/ 目录）
	case "$base" in *_test.rs) continue ;; esac
	case "$f" in */tests/*) continue ;; esac
	total_go="$(grep -c "// Go:" "$f" 2>/dev/null || echo 0)"
	total_go="${total_go//[^0-9]/}"
	[ "${total_go:-0}" -eq 0 ] && continue
	# 区分实现锚 vs 旧式内联测试锚（以同文件内 #[cfg(test)] 起点为界）
	cfgline="$(grep -nE '#\[cfg\(test\)\]' "$f" 2>/dev/null | head -1 | cut -d: -f1 || true)"
	impl_go=0; inline_test_go=0
	if [ -n "$cfgline" ]; then
		impl_go="$(awk -v c="$cfgline" 'NR<c && /\/\/ Go:/{n++} END{print n+0}' "$f")"
		inline_test_go="$(awk -v c="$cfgline" 'NR>=c && /\/\/ Go:/{n++} END{print n+0}' "$f")"
	else
		impl_go="$total_go"
	fi
	[ "$impl_go" -eq 0 ] && continue
	dir="$(dirname "$f")"
	stem="${base%.rs}"
	covered=0
	# 1) 首选：同目录兄弟 <stem>_test.rs 且含 // Go:
	if [ -f "$dir/${stem}_test.rs" ] && grep -q "// Go:" "$dir/${stem}_test.rs" 2>/dev/null; then
		covered=1
	fi
	# 2) 兼容：旧式同文件内联测试锚
	[ "$covered" = "0" ] && [ "$inline_test_go" -gt 0 ] && covered=1
	# 3) 兼容：crate tests/ 目录下引用本文件的测试
	if [ "$covered" = "0" ]; then
		crate_dir="$dir"
		while [ "$crate_dir" != "$REPO_ROOT" ] && [ "$crate_dir" != "/" ]; do
			[ -f "$crate_dir/Cargo.toml" ] && break
			crate_dir="$(dirname "$crate_dir")"
		done
		if [ -d "$crate_dir/tests" ]; then
			while IFS= read -r tf; do
				[ -z "$tf" ] && continue
				if grep -q "// Go:" "$tf" 2>/dev/null; then covered=1; break; fi
			done < <(find "$crate_dir/tests" -type f -name "*${stem}*.rs" 2>/dev/null || true)
		fi
	fi
	if [ "$covered" = "0" ]; then
		echo "  ✗ ${f#"$REPO_ROOT"/}：有 // Go: 锚但缺独立 ${stem}_test.rs（或其无 // Go: 锚）"
		c6_bad=$((c6_bad + 1))
	fi
done < <(collect_rs)
if [ "$c6_bad" -gt 0 ]; then
	note_fail "C6 test-go-parity：$c6_bad 个实现文件缺 Go 对齐测试"
else
	echo "  C6 OK"
fi

# ─── C7 stub-readiness ───────────────────────────────────────────────────────
echo ""; echo "[C7] todo!()/unimplemented!() 须紧邻 // DEFER(phase-N) + // blocked-by:"
c7_bad=0
adjacent_has() {
	# $1=file $2=lineno $3=pattern；检查同行 / 上一行 / 下一行
	local file="$1" ln="$2" pat="$3"
	local a b c
	a="$(sed -n "${ln}p" "$file" 2>/dev/null || true)"
	b="$(sed -n "$((ln - 1))p" "$file" 2>/dev/null || true)"
	c="$(sed -n "$((ln + 1))p" "$file" 2>/dev/null || true)"
	echo "$a$b$c" | grep -q "$pat"
}
while IFS= read -r f; do
	[ -z "$f" ] && continue
	while IFS= read -r m; do
		[ -z "$m" ] && continue
		ln="${m%%:*}"
		ok=1
		if ! adjacent_has "$f" "$ln" 'DEFER(phase-'; then ok=0; fi
		if ! adjacent_has "$f" "$ln" '// blocked-by:'; then ok=0; fi
		if [ "$ok" = "0" ]; then
			echo "  ✗ ${f#"$REPO_ROOT"/}:${ln} stub 缺 // DEFER(phase-N) 或 // blocked-by:"
			c7_bad=$((c7_bad + 1))
		fi
	done < <(grep -nE '\b(todo|unimplemented)!\(' "$f" 2>/dev/null || true)
done < <(collect_rs)
if [ "$c7_bad" -gt 0 ]; then
	note_fail "C7 stub-readiness：$c7_bad 处 stub 缺 DEFER/blocked-by 标注"
else
	echo "  C7 OK"
fi

# ─── C8 comments must be English (no CJK in .rs comments) ────────────────────
echo ""; echo "[C8] 代码注释一律英文（.rs 注释中禁出现 CJK 字符）"
c8_bad=0
# CJK 范围：CJK 统一表意 + 扩展A + 兼容 + 假名 + 全角标点 + 注音/部首
cjk='[\x{3000}-\x{303F}\x{3040}-\x{30FF}\x{3100}-\x{312F}\x{31A0}-\x{31BF}\x{3400}-\x{4DBF}\x{4E00}-\x{9FFF}\x{F900}-\x{FAFF}\x{FF00}-\x{FFEF}]'
while IFS= read -r f; do
	[ -z "$f" ] && continue
	# 启发式：行注释/块注释起始符之后（同行）出现 CJK 即判违规。
	# 仅扫注释，故 "café"/中文测试数据等字符串字面量不受影响（除非该行还带注释）。
	while IFS= read -r m; do
		[ -z "$m" ] && continue
		echo "  ✗ ${f#"$REPO_ROOT"/}:${m%%:*} 注释含 CJK（请改英文）"
		c8_bad=$((c8_bad + 1))
	done < <(rg -n "(//|/\*).*${cjk}" "$f" 2>/dev/null | head -50 || true)
done < <(collect_rs)
if [ "$c8_bad" -gt 0 ]; then
	note_fail "C8 comments-en：$c8_bad 处代码注释含 CJK（改成英文）"
else
	echo "  C8 OK（注释无 CJK）"
fi

# ─── 小结 + 退出码 ───────────────────────────────────────────────────────────
echo ""
echo "----------------------------------------------------------------"
if [ ${#FAILS[@]} -gt 0 ]; then
	echo " 代码 Gate 小结：RED（${#FAILS[@]} 项失败）"
	for x in "${FAILS[@]}"; do echo "  ✗ $x"; done
	echo "----------------------------------------------------------------"
	exit 1
fi
echo " 代码 Gate 小结：GREEN（C1-C8 全通过）"
echo "----------------------------------------------------------------"
exit 0
