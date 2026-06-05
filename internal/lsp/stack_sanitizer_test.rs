use super::*;

// === defeat_generic_secret_regex tests ===

#[test]
fn defeat_secret_regex_inserts_marker_for_signature() {
    let input = "signature(ctx)";
    let result = defeat_generic_secret_regex(input);
    assert_eq!(result, "signatureX_X(ctx)");
}

#[test]
fn defeat_secret_regex_inserts_marker_for_key() {
    let input = "key[index]";
    let result = defeat_generic_secret_regex(input);
    assert_eq!(result, "keyX_X[index]");
}

#[test]
fn defeat_secret_regex_inserts_marker_for_token() {
    let input = "token(foo)";
    let result = defeat_generic_secret_regex(input);
    assert_eq!(result, "tokenX_X(foo)");
}

#[test]
fn defeat_secret_regex_case_insensitive() {
    let input = "TOKEN(bar)";
    let result = defeat_generic_secret_regex(input);
    assert_eq!(result, "TOKENX_X(bar)");
}

#[test]
fn defeat_secret_regex_no_match_leaves_unchanged() {
    let input = "normalFunction(args)";
    let result = defeat_generic_secret_regex(input);
    assert_eq!(result, "normalFunction(args)");
}

#[test]
fn defeat_secret_regex_sig_with_paren() {
    let input = "sig(x)";
    let result = defeat_generic_secret_regex(input);
    assert_eq!(result, "sigX_X(x)");
}

#[test]
fn defeat_secret_regex_pwd_with_dot() {
    let input = "pwd.something";
    let result = defeat_generic_secret_regex(input);
    assert_eq!(result, "pwdX_X.something");
}

// === sanitize_stack_trace tests ===

#[test]
fn sanitize_stack_trace_empty_on_no_marker() {
    let stack = "some random text\nwithout the marker";
    assert_eq!(sanitize_stack_trace(stack), "");
}

#[test]
fn sanitize_stack_trace_strips_before_marker() {
    let stack = "\
goroutine 1 [running]:
runtime/debug.Stack()
\ttypescript-go/internal/lsp/server.handleRequest()
\t/build/path/typescript-go/internal/lsp/server.go:42 +0x1a3";
    let result = sanitize_stack_trace(stack);
    assert!(result.starts_with("(REDACTED FRAME)"));
    assert!(result.contains("typescript-go|>internal|>lsp|>server.handleRequest()"));
}

#[test]
fn sanitize_stack_trace_redacts_non_internal_frames() {
    let stack = "\
runtime/debug.Stack()
\tnet/http.(*conn).serve()
\ttypescript-go/internal/project/session.DoSomething()";
    let result = sanitize_stack_trace(stack);
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines[0], "(REDACTED FRAME)");
    assert_eq!(lines[1], "\t(REDACTED FRAME)");
    assert!(lines[2].contains("typescript-go|>internal|>project|>session.DoSomething()"));
}

#[test]
fn sanitize_stack_trace_strips_hex_offsets() {
    let stack = "\
runtime/debug.Stack()
\ttypescript-go/internal/checker/checker.checkExpression(0xc000abcdef, 0x1) +0x3a5";
    let result = sanitize_stack_trace(stack);
    assert!(!result.contains("+0x"));
    assert!(result.contains("checker.checkExpression()"));
}

#[test]
fn sanitize_stack_trace_strips_goroutine_suffix() {
    let stack = "\
runtime/debug.Stack()
\ttypescript-go/internal/checker/checker.resolve() in goroutine 42";
    let result = sanitize_stack_trace(stack);
    assert!(!result.contains("goroutine"));
    assert!(result.contains("checker.resolve()"));
}

#[test]
fn sanitize_stack_trace_preserves_leading_whitespace() {
    let stack = "\
runtime/debug.Stack()
  typescript-go/internal/ls/hover.getHoverInfo()";
    let result = sanitize_stack_trace(stack);
    let lines: Vec<&str> = result.lines().collect();
    assert!(lines[1].starts_with("  "));
}

#[test]
fn sanitize_stack_trace_defeats_secret_keywords() {
    let stack = "\
runtime/debug.Stack()
\ttypescript-go/internal/ls/signature.getSignatureHelp()";
    let result = sanitize_stack_trace(stack);
    assert!(result.contains("signatureX_X"));
}

// === write_sanitized_module_or_path tests ===

#[test]
fn sanitized_path_replaces_slashes_with_pipe_gt() {
    let mut result = String::new();
    write_sanitized_module_or_path("typescript-go/internal/lsp/server.go:42", &mut result);
    assert_eq!(result, "typescript-go|>internal|>lsp|>server.go:42");
}

#[test]
fn sanitized_path_strips_func_args() {
    let mut result = String::new();
    write_sanitized_module_or_path(
        "typescript-go/internal/checker.doCheck(0xabc, 0x1)",
        &mut result,
    );
    assert_eq!(result, "typescript-go|>internal|>checker.doCheck()");
}

#[test]
fn sanitized_path_handles_no_open_paren() {
    let mut result = String::new();
    write_sanitized_module_or_path("typescript-go/internal/broken)", &mut result);
    assert_eq!(result, "typescript-go|>internal|>???");
}
