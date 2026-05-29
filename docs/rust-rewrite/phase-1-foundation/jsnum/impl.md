# jsnum: 实现方案（impl.md）

**crate**：`tsgo_jsnum`　**目标**：提供与 JavaScript 完全一致的 number 语义（`Number` = JS double）：位运算（ToInt32/ToUint32 + 移位/与或非异或）、取余、幂、`Number.prototype.toString`、`StringToNumber`，以及 `PseudoBigInt`（bigint 字面量解析）。
**依赖（crate）**：`tsgo_stringutil`（数字字符判定）、`tsgo_json`（float 序列化兜底）。外部：`num-bigint`（大整数）、JS 兼容的 dtoa（见偏离）。
**Go 源**：`internal/jsnum/`（3 个非测试文件：`jsnum.go` 178、`string.go` 342、`pseudobigint.go` 67 行）

## 这个包是什么（业务说明）

evaluator / checker 在常量折叠、字面量类型、枚举值计算时必须严格复现 JS 的数值行为（否则类型与诊断会和 tsc 不一致）。`jsnum` 就是这层"JS 数值语义"封装：

- **`Number`（= `float64`）**：所有可直接做的运算（转换、算术）按 JS 行为，其它运算走本类型的方法（不要直接用 `math` 包）。
- **位运算**：`ToInt32`/`ToUint32`（ECMAScript 抽象操作）+ `SignedRightShift`/`UnsignedRightShift`/`LeftShift`/`BitwiseNOT`/`BitwiseOR`/`BitwiseAND`/`BitwiseXOR`（移位计数 `& 31`）。
- **算术**：`Remainder`（JS `%`，用 IEEE 754 fmod 而非手写公式以避免误差）、`Exponentiate`（JS `**`，大整数指数用 `big.Int` 精确 + round-to-nearest-even，保证 ≤1 ULP）、`Floor`/`Abs`/`trunc`。
- **字符串**：`String()`（`Number.prototype.toString`，安全整数走快路径，其余走 JSON float 序列化）、`FromString`（`StringToNumber`，处理空白/`Infinity`/进制前缀 `0b/0o/0x`/小数/指数）。
- **`PseudoBigInt`**：bigint 字面量（`123n`、`0x1Fn` 等）解析为"符号 + base10 绝对值字符串"。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `type Number float64` | `pub struct Number(f64)`（newtype，`#[derive(Copy,Clone,PartialOrd)]`） | newtype 防止误用普通 f64 运算；`PartialEq` 需小心 NaN/±0 语义 |
| `math.NaN/IsNaN/Inf/IsInf/Trunc/Mod/Pow/Floor/Abs/Copysign` | `f64::NAN/is_nan/INFINITY/is_infinite/trunc/`(`%`/`rem_euclid` 慎选)`/powf/floor/abs/copysign` | 注意 `Mod` = IEEE fmod = Rust `%`（对 f64 是 fmod 语义，需确认）；`Copysign` → `f64::copysign` |
| `toInt32`（ECMAScript ToInt32） | `fn to_int32(self) -> i32` | SMI 快路径 + 非有限→0 + `trunc` + `mod 2^32` + 有符号回绕；逐分支照搬 |
| `toUint32` | `fn to_uint32(self) -> u32`（= `to_int32 as u32`） | |
| `toShiftCount` | `& 31` | |
| `math.Float64bits/frombits` | `f64::to_bits/from_bits` | `isNonFinite` 用位掩码 `0x7FF0...` |
| `big.Int`（Exponentiate 精确路径 / 大整数解析） | `num_bigint::BigInt` | `Exp`/`SetString(_,0)`/`Float64` → num-bigint 对应 |
| `big.Float.SetPrec(256).SetInt(ri).Float64()` | `num-bigint` → f64 的正确舍入（round-to-nearest-even） | 需保证 256-bit 中间精度的等价舍入；存疑加注释 |
| `Number.toString` 非快路径用 `json.Marshal(float64)` | **JS 兼容 dtoa**（见偏离） | Go 的 json float 格式≈JS；Rust 需匹配 ECMAScript Number::toString 输出（`1e+308`/`1e+21`/`100000000000000000000`） |
| `PseudoBigInt{Negative bool; Base10Value string}` | `struct PseudoBigInt { negative: bool, base10_value: String }` | 零值（空字符串）= 0 |
| `strconv.ParseInt/ParseFloat` | `i64::from_str_radix` / `str::parse::<f64>()` | 注意 `ParseFloat` 的 `ErrRange` → Rust `parse` 对超范围返回 `inf`（语义需对齐 `stringToFloat64`） |
| `panic`（ParsePseudoBigInt 解析失败） | `panic!` | 同语义 |

> **命门 1（dtoa）**：`Number.toString` 必须逐字节匹配 ECMAScript 的 Number-to-String 算法（测试 `ryu_test.go` 来自 Ryu 算法语料）。Go 用 json float 格式恰好≈JS；Rust 的 `ryu` crate 给最短往返但**指数格式不同**（如不会输出 `1e+21` 风格）。需移植一个 JS 兼容的 dtoa（或在 ryu 输出上做格式转换层）。这是本包最难点，标 `// TODO(port)` 并由 `TestString`/`TestStringRoundtrip` 的 ~180 个字面量 gate。
> **命门 2（Exponentiate）**：大整数幂用 `big.Int` 精确 + 正确舍入，保证与 V8 ≤1 ULP（`TestExponentiate` 的 `numberFromBits(...)` 期望值是精确舍入结果）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/jsnum/jsnum.go` | `internal/jsnum/jsnum.rs`（basename == crate 目录名 → `lib.rs`） | Number 类型 + 位运算/算术 |
| `internal/jsnum/string.go` | `internal/jsnum/string.rs` | `String()` + `FromString` + 解析辅助 |
| `internal/jsnum/pseudobigint.go` | `internal/jsnum/pseudobigint.rs` | `PseudoBigInt` |
| （crate 根） | `internal/jsnum/lib.rs` | 声明子模块 + re-export |

## 依赖白名单（本包新增的 crate）

- `num-bigint`（大整数：Exponentiate 精确路径、`tryParseInt`/`ParsePseudoBigInt` 的大数解析）。
- JS 兼容 dtoa：优先评估 `ryu` + 自写格式层；或移植 ECMAScript Number::toString 算法。记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `lib.rs`（Go: `internal/jsnum/jsnum.go`）

- [x] `pub struct Number(f64)` + `MAX_SAFE_INTEGER`(2^53-1) / `MIN_SAFE_INTEGER`　`// Go: jsnum.go:Number/MaxSafeInteger/MinSafeInteger`
- [x] `nan() / is_nan() / inf(sign) / is_inf()` + `is_non_finite(f64)`（位掩码）　`// Go: jsnum.go:NaN/IsNaN/Inf/IsInf/isNonFinite`
- [x] `fn to_uint32(self) -> u32` / `fn to_int32(self) -> i32`（SMI 快路径 + trunc + mod 2^32 + 回绕）/ `fn to_shift_count(self) -> u32`　`// Go: jsnum.go:toUint32/toInt32/toShiftCount`
- [x] `signed_right_shift / unsigned_right_shift / left_shift / bitwise_not / bitwise_or / bitwise_and / bitwise_xor`　`// Go: jsnum.go:SignedRightShift/UnsignedRightShift/LeftShift/BitwiseNOT/BitwiseOR/BitwiseAND/BitwiseXOR`
- [x] `trunc / floor / abs` + `negative_zero`　`// Go: jsnum.go:trunc/Floor/Abs/negativeZero`
- [x] `remainder(d)`（NaN/Inf/0 特例 + `fmod`）　`// Go: jsnum.go:Remainder`
- [x] `exponentiate(exponent)`（base==±1 特例 + 大整数精确路径 + `powf` 兜底）　`// Go: jsnum.go:Exponentiate`

### `string.rs`（Go: `internal/jsnum/string.go`）

- [x] `impl Display for Number`（NaN/±Infinity → 安全整数快路径 → JS 兼容 dtoa）　`// Go: string.go:(Number).String`
- [x] `pub fn from_string(s: &str) -> Number`（trim JS 空白 → 空/Infinity 特例 → 字符校验 → tryParseInt → 符号 → parseFloatString）　`// Go: string.go:FromString`
- [x] `is_str_white_space(char) -> bool`（JS WhiteSpace+LineTerminator，含 `Zs`，**不同于** stringutil 版）　`// Go: string.go:isStrWhiteSpace`
- [x] `try_parse_int(s) -> Option<Number>`（`0b/0B/0o/0O/0x/0X` 进制 + 十进制去前导零 + 大数 big.Int）　`// Go: string.go:tryParseInt`
- [x] `parse_float_string(s) -> f64`（拆 `<a>.<b>e<c>` 重组喂给 `parse::<f64>`）　`// Go: string.go:parseFloatString`
- [x] 辅助：`cut_any / trim_leading_zeros / trim_trailing_zeros / string_to_float64 / is_all_digits / is_all_binary_digits / is_all_octal_digits / is_all_hex_digits / is_number_rune`　`// Go: string.go:*`

### `pseudobigint.rs`（Go: `internal/jsnum/pseudobigint.go`）

- [x] `pub struct PseudoBigInt { negative: bool, base10_value: String }` + `new(value, negative)`（去前导零；负仅当非空）　`// Go: pseudobigint.go:PseudoBigInt/NewPseudoBigInt`
- [x] `impl Display`（空→`"0"`，负→`"-"+v`）/ `sign() -> i32`　`// Go: pseudobigint.go:String/Sign`
- [x] `parse_valid_big_int(text) -> PseudoBigInt`（剥 `-` → ParsePseudoBigInt）　`// Go: pseudobigint.go:ParseValidBigInt`
- [x] `parse_pseudo_big_int(s) -> String`（剥尾 `n`；十进制去前导零；非十进制用 big.Int 转十进制；失败 panic）　`// Go: pseudobigint.go:ParsePseudoBigInt`

### Cargo / crate 接线

- [x] `internal/jsnum/Cargo.toml`（`name = "tsgo_jsnum"`，deps `tsgo_stringutil` `tsgo_json` path + `num-bigint`）
- [x] 根 `Cargo.toml` workspace members 追加
- [x] `lib.rs` 声明子模块 + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `to_int32` / `to_uint32`（位运算基石；`TestToInt32` 47 个字面量 gate）。
2. 位运算族（`TestBitwise*` / `TestSignedRightShift` / `TestUnsignedRightShift` / `TestLeftShift`，全小表）。
3. `remainder`（`TestRemainder` 含 NaN/Inf/±0 特例 + fmod 误差用例）。
4. `exponentiate`（`TestExponentiate` 含 ±1/Inf 特例 + 大整数精确 ULP 用例）。
5. `Display`（JS dtoa；`TestString` + `ryu` 语料 ~180 个，命门）。
6. `from_string`（`TestFromString`：stringTests + fromStringTests ~120 个）+ `TestStringRoundtrip`。
7. `pseudo_big_int`（`TestParsePseudoBigInt`：十进制去零 + 进制 + 大字面量）。

## 与 Go 的已知偏离（divergence）

- **dtoa**：Go 用 `json.Marshal(float64)` 兜底 `Number.toString`；Rust 需 JS 兼容 dtoa（ryu + 格式层 或 移植 ECMAScript 算法）。`// TODO(port)`，由 ~180 个字面量 gate。
- **`big.Float` 256-bit 中间精度舍入**：Rust 用 `num-bigint` → f64 需复现 round-to-nearest-even；存疑加注释。
- **`strconv.ParseFloat` 的 ErrRange**：Go 在超范围时返回 `±Inf`（`stringToFloat64` 特判 `ErrRange`）。Rust `str::parse::<f64>()` 对超大值返回 `inf`、无范围错误，语义需逐用例对齐（`"1e1000"`→Inf）。
- **Node 对拍子测试**：`TestToInt32/Node`、`TestString JS`、`Fuzz*JS` 等需真实 Node.js 运行（`jstest.SkipIfNoNodeJS`）。Rust 侧这些归 P10 parity / 可选集成（需 Node），不在 P1 核心单测里强制。
- **`negativeZero`**：JS 区分 +0/-0（`String(-0)=="0"` 但 `FromString("-0")` 得 -0）。Rust f64 同样区分；`PartialEq` 下 `0.0==-0.0` 为真，需用 `to_bits` 或 `is_sign_negative` 在相关用例区分。

## 转交 / 推迟（DEFER）

- 全部 `Node` 子测试与 `Fuzz*JS`（对拍真实 V8）→ P10 parity / 实现期可选集成测试（需 Node）。
- evaluator 对本包的真实调用（常量折叠/枚举）→ P4 接通。
