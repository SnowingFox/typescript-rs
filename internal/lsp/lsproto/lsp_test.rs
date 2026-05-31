use super::*;
use crate::DocumentUri;

/// Asserts that `DocumentUri(uri).file_name() == file_name`.
// Go: internal/ls/lsconv/converters_test.go:TestDocumentURIToFileName
fn assert_file_name(uri: &str, file_name: &str) {
    assert_eq!(DocumentUri(uri.to_string()).file_name(), file_name);
}

// Go: .../TestDocumentURIToFileName/"file:///path/to/file.ts"
#[test]
fn file_name_posix_path() {
    assert_file_name("file:///path/to/file.ts", "/path/to/file.ts");
}

// Go: .../TestDocumentURIToFileName/"file://server/share/file.ts"
#[test]
fn file_name_unc_host() {
    assert_file_name("file://server/share/file.ts", "//server/share/file.ts");
}

// Go: .../TestDocumentURIToFileName/"file://shares/files/c%23/p.cs"
#[test]
fn file_name_host_percent_decoded_path() {
    assert_file_name("file://shares/files/c%23/p.cs", "//shares/files/c#/p.cs");
}

// Go: .../TestDocumentURIToFileName/"file://localhost/c%24/GitDevelopment/express"
#[test]
fn file_name_host_localhost_percent_decoded() {
    assert_file_name(
        "file://localhost/c%24/GitDevelopment/express",
        "//localhost/c$/GitDevelopment/express",
    );
}

// Go: .../TestDocumentURIToFileName/"file:///c:/test/me"
#[test]
fn file_name_windows_drive_plain() {
    assert_file_name("file:///c:/test/me", "c:/test/me");
}

// Go: .../TestDocumentURIToFileName/"file:///d%3A/work/tsgo932/lib/utils.ts"
#[test]
fn file_name_windows_drive_encoded() {
    assert_file_name(
        "file:///d%3A/work/tsgo932/lib/utils.ts",
        "d:/work/tsgo932/lib/utils.ts",
    );
}

// Go: .../TestDocumentURIToFileName/"file:///D%3A/work/tsgo932/lib/utils.ts"
#[test]
fn file_name_windows_drive_uppercase_normalized() {
    assert_file_name(
        "file:///D%3A/work/tsgo932/lib/utils.ts",
        "d:/work/tsgo932/lib/utils.ts",
    );
}

// Go: .../TestDocumentURIToFileName/"file:///d%3A/work/tsgo932/app/%28test%29/comp/comp-test.tsx"
#[test]
fn file_name_windows_drive_parens() {
    assert_file_name(
        "file:///d%3A/work/tsgo932/app/%28test%29/comp/comp-test.tsx",
        "d:/work/tsgo932/app/(test)/comp/comp-test.tsx",
    );
}

// Go: .../TestDocumentURIToFileName/"file:///c:/Source/Z%C3%BCrich..."
#[test]
fn file_name_windows_drive_unicode() {
    assert_file_name(
        "file:///c:/Source/Z%C3%BCrich%20or%20Zurich%20(%CB%88zj%CA%8A%C9%99r%C9%AAk,/Code/resources/app/plugins/c%23/plugin.json",
        "c:/Source/Zürich or Zurich (ˈzjʊərɪk,/Code/resources/app/plugins/c#/plugin.json",
    );
}

// Go: .../TestDocumentURIToFileName/"file:///c:/test %25/path"
#[test]
fn file_name_windows_drive_literal_space_and_percent() {
    assert_file_name("file:///c:/test %25/path", "c:/test %/path");
}

// Go: .../TestDocumentURIToFileName/"file:///c%3A/test%20with%20%2525/c%23code"
#[test]
fn file_name_windows_drive_double_encoded_percent() {
    assert_file_name(
        "file:///c%3A/test%20with%20%2525/c%23code",
        "c:/test with %25/c#code",
    );
}

// Go: .../TestDocumentURIToFileName/"file:///_:/path"
#[test]
fn file_name_non_volume_colon_kept() {
    assert_file_name("file:///_:/path", "/_:/path");
}

// Go: .../TestDocumentURIToFileName/"file:///users/me/c%23-projects/"
#[test]
fn file_name_trailing_slash_no_volume() {
    assert_file_name("file:///users/me/c%23-projects/", "/users/me/c#-projects/");
}

// Go: .../TestDocumentURIToFileName/"file:///path/to/file.ts#section"
#[test]
fn file_name_strips_fragment() {
    assert_file_name("file:///path/to/file.ts#section", "/path/to/file.ts");
}

// Go: .../TestDocumentURIToFileName/"untitled:Untitled-1"
#[test]
fn file_name_untitled_simple() {
    assert_file_name(
        "untitled:Untitled-1",
        "^/untitled/ts-nul-authority/Untitled-1",
    );
}

// Go: .../TestDocumentURIToFileName/"untitled:Untitled-1#fragment"
#[test]
fn file_name_untitled_keeps_fragment() {
    assert_file_name(
        "untitled:Untitled-1#fragment",
        "^/untitled/ts-nul-authority/Untitled-1#fragment",
    );
}

// Go: .../TestDocumentURIToFileName/"untitled:c:/Users/jrieken/Code/abc.txt"
#[test]
fn file_name_untitled_with_drive() {
    assert_file_name(
        "untitled:c:/Users/jrieken/Code/abc.txt",
        "^/untitled/ts-nul-authority/c:/Users/jrieken/Code/abc.txt",
    );
}

// Go: .../TestDocumentURIToFileName/"untitled:C:/Users/jrieken/Code/abc.txt"
#[test]
fn file_name_untitled_with_drive_uppercase_kept() {
    assert_file_name(
        "untitled:C:/Users/jrieken/Code/abc.txt",
        "^/untitled/ts-nul-authority/C:/Users/jrieken/Code/abc.txt",
    );
}

// Go: .../TestDocumentURIToFileName/"untitled://wsl%2Bubuntu/home/.../newfile.ts"
#[test]
fn file_name_untitled_with_authority() {
    assert_file_name(
        "untitled://wsl%2Bubuntu/home/jabaile/work/TypeScript-go/newfile.ts",
        "^/untitled/wsl%2Bubuntu/home/jabaile/work/TypeScript-go/newfile.ts",
    );
}

// Go: internal/lsp/lsproto/lsp.go:URI (serializes as a plain JSON string)
#[test]
fn uri_string_round_trip() {
    let u: URI = serde_json::from_str("\"https://example.com/x\"").unwrap();
    assert_eq!(u.0, "https://example.com/x");
    assert_eq!(
        serde_json::to_string(&u).unwrap(),
        "\"https://example.com/x\""
    );
}

// Go: internal/lsp/lsproto/lsp.go:DocumentUri.Path (case-insensitive lowercases)
#[test]
fn path_case_insensitive_lowercases() {
    let uri = DocumentUri("file:///Users/Me/File.TS".to_string());
    assert_eq!(uri.path(false).as_str(), "/users/me/file.ts");
}

// Go: internal/lsp/lsproto/lsp.go:DocumentUri.Path (case-sensitive preserves case)
#[test]
fn path_case_sensitive_preserves_case() {
    let uri = DocumentUri("file:///Users/Me/File.TS".to_string());
    assert_eq!(uri.path(true).as_str(), "/Users/Me/File.TS");
}
