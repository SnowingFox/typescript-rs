//! `tsgo_debug` — compiler-internal assertion/failure primitives (reporting
//! "should never happen" states via panic).
//!
//! 1:1 port of Go `internal/debug/debug.go`. These assertion failures indicate
//! a compiler bug (not user input error), so they are implemented with `panic!`
//! and the panic message is kept byte-for-byte aligned with Go.
//!
//! # Divergence from Go
//! - Go `AssertNever(member any, ...)` picks the detail at runtime via a
//!   `KindString -> Stringer -> %v` cascade. Rust has no runtime trait
//!   reflection, so [`assert_never`] uniformly takes the detail via
//!   [`std::fmt::Display`]; types that want to expose `KindString` should also
//!   implement `Display`. The [`KindString`] trait is still used by
//!   [`fail_bad_syntax_kind`] (for AST nodes).
//! - Go `message ...any` (variadic) is currently expressed as `Option<&str>`
//!   (tests use only a single argument).

/// Trait providing a value's "kind name", used by [`fail_bad_syntax_kind`].
///
/// Corresponds to Go's `interface{ KindString() string }`; implementors are
/// typically AST nodes (landing in P2/P3).
///
/// # Examples
/// ```
/// use tsgo_debug::KindString;
/// struct Node;
/// impl KindString for Node {
///     fn kind_string(&self) -> String { "Identifier".to_string() }
/// }
/// assert_eq!(Node.kind_string(), "Identifier");
/// ```
pub trait KindString {
    /// Returns this value's kind name (e.g. an AST node's `SyntaxKind` string).
    fn kind_string(&self) -> String;
}

/// Fails unconditionally: panics with a `"Debug failure. "` prefix.
///
/// Empty `reason` -> `"Debug failure."`; otherwise `"Debug failure. {reason}"`.
///
/// # Examples
/// ```should_panic
/// tsgo_debug::fail("boom");
/// ```
///
/// Side effects: triggers `panic!` (never returns).
// Go: internal/debug/debug.go:Fail
pub fn fail(reason: &str) -> ! {
    let msg = if reason.is_empty() {
        "Debug failure.".to_string()
    } else {
        format!("Debug failure. {reason}")
    };
    panic!("{msg}");
}

/// Fails on encountering an unexpected AST node kind, appending
/// `node.kind_string()`.
///
/// Default message `"Unexpected node."`; format `"{msg}\nNode {kind} was
/// unexpected."`.
///
/// # Examples
/// ```should_panic
/// use tsgo_debug::{fail_bad_syntax_kind, KindString};
/// struct N;
/// impl KindString for N { fn kind_string(&self) -> String { "Foo".into() } }
/// fail_bad_syntax_kind(&N, None);
/// ```
///
/// Side effects: triggers `panic!` (never returns).
// Go: internal/debug/debug.go:FailBadSyntaxKind
pub fn fail_bad_syntax_kind(node: &impl KindString, message: Option<&str>) -> ! {
    let msg = message.unwrap_or("Unexpected node.");
    fail(&format!(
        "{msg}\nNode {} was unexpected.",
        node.kind_string()
    ));
}

/// Exhaustiveness check fallback (corresponds to TS `assertNever`): prints the
/// illegal value's [`Display`] form.
///
/// Default message `"Illegal value:"`; format `"{msg} {detail}"`.
///
/// # Examples
/// ```should_panic
/// tsgo_debug::assert_never(42, None);
/// ```
///
/// Side effects: triggers `panic!` (never returns).
///
/// [`Display`]: std::fmt::Display
// Go: internal/debug/debug.go:AssertNever
pub fn assert_never<T: std::fmt::Display>(member: T, message: Option<&str>) -> ! {
    let msg = message.unwrap_or("Illegal value:");
    fail(&format!("{msg} {member}"));
}

/// Conditional assertion: fails when `value` is false.
///
/// # Examples
/// ```
/// tsgo_debug::assert(1 + 1 == 2, None);
/// ```
/// ```should_panic
/// tsgo_debug::assert(false, Some("x must be positive"));
/// ```
///
/// Side effects: triggers `panic!` when `value` is false, otherwise none.
// Go: internal/debug/debug.go:Assert
pub fn assert(value: bool, message: Option<&str>) {
    if value {
        return;
    }
    assert_slow(message);
}

/// Slow path for a failed `assert`: assembles the message and calls [`fail`].
///
/// With a message -> `"False expression: {msg}"`; otherwise `"False
/// expression."`.
// Go: internal/debug/debug.go:assertSlow
fn assert_slow(message: Option<&str>) -> ! {
    let msg = match message {
        Some(m) => format!("False expression: {m}"),
        None => "False expression.".to_string(),
    };
    fail(&msg);
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
