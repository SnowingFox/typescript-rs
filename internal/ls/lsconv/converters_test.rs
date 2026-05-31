use super::*;
use std::io::Write;
use std::process::{Command, Stdio};
use std::rc::Rc;
use tsgo_core::text::{TextPos, TextRange};
use tsgo_lsproto::{DocumentUri, Location, Position, Range};

use crate::linemap::compute_lsp_line_starts;

fn uri(s: &str) -> DocumentUri {
    DocumentUri(s.to_string())
}

struct TestScript {
    name: String,
    text: Vec<u8>,
}

impl Script for TestScript {
    fn file_name(&self) -> &str {
        &self.name
    }
    fn text(&self) -> &[u8] {
        &self.text
    }
}

// Go: internal/ls/lsconv/converters_test.go:newTestConverters
fn new_test_converters(text: &[u8]) -> (Converters, TestScript) {
    let script = TestScript {
        name: "test.ts".to_string(),
        text: text.to_vec(),
    };
    let line_map = Rc::new(compute_lsp_line_starts(text));
    let conv = Converters::new(PositionEncodingKind::utf16(), move |_| line_map.clone());
    (conv, script)
}

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

// Tracer: a simple POSIX path becomes a `file://` URI.
// Go: internal/ls/lsconv/converters_test.go:TestFileNameToDocumentURI/"/path/to/file.ts"
#[test]
fn file_name_to_document_uri_simple_posix_path() {
    assert_eq!(
        file_name_to_document_uri("/path/to/file.ts"),
        uri("file:///path/to/file.ts")
    );
}

// Full 1:1 port of the Go table; each row is a sub-case.
// Go: internal/ls/lsconv/converters_test.go:TestFileNameToDocumentURI
#[test]
fn file_name_to_document_uri_table() {
    let cases: &[(&str, &str)] = &[
        ("/path/to/file.ts", "file:///path/to/file.ts"),
        ("//server/share/file.ts", "file://server/share/file.ts"),
        (
            "d:/work/tsgo932/lib/utils.ts",
            "file:///d%3A/work/tsgo932/lib/utils.ts",
        ),
        (
            "d:/work/tsgo932/lib/utils.ts",
            "file:///d%3A/work/tsgo932/lib/utils.ts",
        ),
        (
            "d:/work/tsgo932/app/(test)/comp/comp-test.tsx",
            "file:///d%3A/work/tsgo932/app/%28test%29/comp/comp-test.tsx",
        ),
        ("/path/to/file.ts", "file:///path/to/file.ts"),
        ("c:/test/me", "file:///c%3A/test/me"),
        ("//shares/files/c#/p.cs", "file://shares/files/c%23/p.cs"),
        (
            "c:/Source/Zürich or Zurich (ˈzjʊərɪk,/Code/resources/app/plugins/c#/plugin.json",
            "file:///c%3A/Source/Z%C3%BCrich%20or%20Zurich%20%28%CB%88zj%CA%8A%C9%99r%C9%AAk%2C/Code/resources/app/plugins/c%23/plugin.json",
        ),
        ("c:/test %/path", "file:///c%3A/test%20%25/path"),
        ("/", "file:///"),
        ("/_:/path", "file:///_%3A/path"),
        ("/users/me/c#-projects/", "file:///users/me/c%23-projects/"),
        (
            "//localhost/c$/GitDevelopment/express",
            "file://localhost/c%24/GitDevelopment/express",
        ),
        (
            "c:/test with %25/c#code",
            "file:///c%3A/test%20with%20%2525/c%23code",
        ),
        ("^/untitled/ts-nul-authority/Untitled-1", "untitled:Untitled-1"),
        (
            "^/untitled/ts-nul-authority/c:/Users/jrieken/Code/abc.txt",
            "untitled:c:/Users/jrieken/Code/abc.txt",
        ),
        (
            "^/untitled/ts-nul-authority///wsl%2Bubuntu/home/jabaile/work/TypeScript-go/newfile.ts",
            "untitled://wsl%2Bubuntu/home/jabaile/work/TypeScript-go/newfile.ts",
        ),
    ];
    for (file_name, expected) in cases {
        assert_eq!(
            file_name_to_document_uri(file_name),
            uri(expected),
            "FileNameToDocumentURI({file_name:?})"
        );
    }
}

// Tracer: byte offset -> (line, UTF-16 character) over a BMP-non-ASCII text,
// forcing the UTF-16 rescan branch (ascii_only = false).
// Go: internal/ls/lsconv/converters.go:PositionToLineAndCharacter
#[test]
fn position_to_line_and_character_bmp() {
    let (conv, script) = new_test_converters("α\nβ".as_bytes());
    assert_eq!(
        conv.position_to_line_and_character(&script, TextPos(0)),
        pos(0, 0)
    );
    assert_eq!(
        conv.position_to_line_and_character(&script, TextPos(2)),
        pos(0, 1)
    );
    assert_eq!(
        conv.position_to_line_and_character(&script, TextPos(3)),
        pos(1, 0)
    );
}

// Tracer: (line, UTF-16 character) -> byte offset, inverse of the above.
// Go: internal/ls/lsconv/converters.go:LineAndCharacterToPosition
#[test]
fn line_and_character_to_position_bmp() {
    let (conv, script) = new_test_converters("α\nβ".as_bytes());
    assert_eq!(
        conv.line_and_character_to_position(&script, pos(0, 0)),
        TextPos(0)
    );
    assert_eq!(
        conv.line_and_character_to_position(&script, pos(0, 1)),
        TextPos(2)
    );
    assert_eq!(
        conv.line_and_character_to_position(&script, pos(1, 0)),
        TextPos(3)
    );
}

// Behavior on text containing invalid UTF-8 (a lone continuation byte 0x80).
// Each invalid byte advances the byte position by 1 and the UTF-16 character by
// 1 (RuneError = 1 code unit).
// Go: internal/ls/lsconv/converters_test.go:TestConvertersInvalidUTF8
#[test]
fn converters_invalid_utf8() {
    // Text with invalid UTF-8 byte 0x80 (continuation byte without start byte).
    let text: &[u8] = b"a\x80b\ncd";
    let (conv, script) = new_test_converters(text);

    // (line, char) -> byte position, asserted in both directions.
    let mappings: &[(u32, u32, i32)] = &[
        (0, 0, 0), // 'a'
        (0, 1, 1), // invalid byte 0x80
        (0, 2, 2), // 'b'
        (0, 3, 3), // newline (line end)
        (1, 0, 4), // 'c'
        (1, 1, 5), // 'd'
        (1, 2, 6), // EOF
    ];
    for &(line, char, byte_pos) in mappings {
        let lc = pos(line, char);
        assert_eq!(
            conv.line_and_character_to_position(&script, lc.clone()),
            TextPos(byte_pos),
            "LineAndCharacterToPosition({line},{char})"
        );
        assert_eq!(
            conv.position_to_line_and_character(&script, TextPos(byte_pos)),
            lc,
            "PositionToLineAndCharacter({byte_pos})"
        );
    }

    // Byte-by-byte round-trip across the entire text.
    for byte_pos in 0..=(text.len() as i32) {
        let lc = conv.position_to_line_and_character(&script, TextPos(byte_pos));
        let rt = conv.line_and_character_to_position(&script, lc);
        assert_eq!(rt, TextPos(byte_pos), "round-trip byte {byte_pos}");
    }
}

// Behavior test (no direct Go test): a TextRange converts to an LSP Range and
// back, exercising both endpoints and the line split.
// Go: internal/ls/lsconv/converters.go:ToLSPRange / FromLSPRange
#[test]
fn to_and_from_lsp_range_roundtrip() {
    let (conv, script) = new_test_converters(b"ab\ncd");
    let r = conv.to_lsp_range(&script, TextRange::new(0, 5));
    assert_eq!(
        r,
        Range {
            start: pos(0, 0),
            end: pos(1, 2)
        }
    );
    let back = conv.from_lsp_range(&script, r);
    assert_eq!((back.pos(), back.end()), (0, 5));
}

// Behavior test (no direct Go test): ToLSPLocation pairs the file URI with the
// converted range.
// Go: internal/ls/lsconv/converters.go:ToLSPLocation
#[test]
fn to_lsp_location_builds_uri_and_range() {
    let line_map = Rc::new(compute_lsp_line_starts(b"ab\ncd"));
    let conv = Converters::new(PositionEncodingKind::utf16(), move |_| line_map.clone());
    let script = TestScript {
        name: "/a/b.ts".to_string(),
        text: b"ab\ncd".to_vec(),
    };
    let loc = conv.to_lsp_location(&script, TextRange::new(0, 5));
    assert_eq!(
        loc,
        Location {
            uri: uri("file:///a/b.ts"),
            range: Range {
                start: pos(0, 0),
                end: pos(1, 2),
            },
        }
    );
}

// jsReferenceScript is a Node.js script that, given a list of UTF-8 byte
// buffers, computes the authoritative mapping between (line, character in UTF-16
// code units) and UTF-8 byte offsets. See the Go test for the full rationale.
// Go: internal/ls/lsconv/converters_test.go:jsReferenceScript
const JS_REFERENCE_SCRIPT: &str = r#"
const inChunks = [];
process.stdin.on('data', c => inChunks.push(c));
process.stdin.on('end', () => {
  const buf = Buffer.concat(inChunks);
  let off = 0;
  const readU32 = () => { const v = buf.readUInt32LE(off); off += 4; return v; };
  const n = readU32();
  const buffers = [];
  for (let i = 0; i < n; i++) {
    const len = readU32();
    buffers.push(buf.subarray(off, off + len));
    off += len;
  }

  const decoder = new TextDecoder('utf-8', { fatal: true });
  const out = buffers.map(bytes => {
    const text = decoder.decode(bytes);

    const lineStartsJs = [0];
    for (let i = 0; i < text.length; i++) {
      const c = text.charCodeAt(i);
      if (c === 13) {
        if (i + 1 < text.length && text.charCodeAt(i + 1) === 10) i++;
        lineStartsJs.push(i + 1);
      } else if (c === 10) {
        lineStartsJs.push(i + 1);
      }
    }

    const boundaries = [{ bytePos: 0, jsIdx: 0 }];
    let bytePos = 0, jsIdx = 0;
    while (bytePos < bytes.length) {
      const seq = utf8SeqLen(bytes[bytePos]);
      const cp = text.codePointAt(jsIdx);
      bytePos += seq;
      jsIdx += cp > 0xFFFF ? 2 : 1;
      boundaries.push({ bytePos, jsIdx });
    }

    return boundaries.map(({ bytePos, jsIdx }) => {
      let lo = 0, hi = lineStartsJs.length - 1;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (lineStartsJs[mid] <= jsIdx) lo = mid;
        else hi = mid - 1;
      }
      return { bytePos, line: lo, char: jsIdx - lineStartsJs[lo] };
    });
  });

  process.stdout.write(JSON.stringify(out));
});

function utf8SeqLen(b) {
  if (b < 0x80) return 1;
  if ((b & 0xE0) === 0xC0) return 2;
  if ((b & 0xF0) === 0xE0) return 3;
  if ((b & 0xF8) === 0xF0) return 4;
  throw new Error('invalid UTF-8 lead byte 0x' + b.toString(16));
}
"#;

#[derive(serde::Deserialize)]
struct JsTuple {
    #[serde(rename = "bytePos")]
    byte_pos: i32,
    line: i32,
    char: i32,
}

// Returns `None` when `node` is unavailable (test should skip), mirroring the
// Go test's `t.Skipf`.
// Go: internal/ls/lsconv/converters_test.go:runJSReference
fn run_js_reference(texts: &[&str]) -> Option<Vec<Vec<JsTuple>>> {
    // Length-prefixed binary stream: [u32 LE count] then [u32 LE len][bytes]...
    let mut input: Vec<u8> = Vec::new();
    input.extend_from_slice(&(texts.len() as u32).to_le_bytes());
    for s in texts {
        let bytes = s.as_bytes();
        input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        input.extend_from_slice(bytes);
    }

    let mut child = Command::new("node")
        .arg("-e")
        .arg(JS_REFERENCE_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        stdin.write_all(&input).expect("write node stdin");
    }
    let output = child.wait_with_output().expect("node wait");
    assert!(
        output.status.success(),
        "node failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: Vec<Vec<JsTuple>> =
        serde_json::from_slice(&output.stdout).expect("parse node output");
    Some(parsed)
}

// Cross-checks the Rust UTF-16 conversions against authoritative results from
// Node.js using real UTF-16 string semantics.
// Go: internal/ls/lsconv/converters_test.go:TestConvertersAgainstJSReference
#[test]
fn converters_against_js_reference() {
    let cases: &[(&str, &str)] = &[
        ("empty", ""),
        ("ascii", "hello\nworld"),
        ("ascii_crlf", "hello\r\nworld\r\n!"),
        ("ascii_cr_only", "a\rb\rc"),
        ("trailing_newline", "abc\n"),
        ("bmp_em_dash", "ab\u{2014}cd\nef"),
        ("bmp_multi", "α\nβ\nγδε\nzz"),
        ("supplementary_emoji", "x\u{1F600}y\nz"),
        ("supplementary_at_lineend", "ab\u{1F600}\ncd\u{1F60A}"),
        ("supplementary_only", "\u{1F600}\u{1F601}\u{1F602}"),
        ("mixed", "α \u{2014} \u{1F600}\r\nβ\nγ\r"),
        ("long_mixed_ws", "  \tαβ\n\t\u{1F600}  end\n"),
        ("zwj_emoji", "\u{1F468}\u{200D}\u{1F4BB}\nnext"),
        ("only_newlines", "\n\n\r\n\r"),
    ];

    let texts: Vec<&str> = cases.iter().map(|&(_, t)| t).collect();
    let Some(refs) = run_js_reference(&texts) else {
        eprintln!("node not available: skipping converters_against_js_reference");
        return;
    };
    assert_eq!(refs.len(), cases.len());

    for (i, &(name, text)) in cases.iter().enumerate() {
        let reference = &refs[i];
        let (conv, script) = new_test_converters(text.as_bytes());
        for tup in reference {
            let byte_pos = TextPos(tup.byte_pos);
            let expected_lc = pos(tup.line as u32, tup.char as u32);

            let got_lc = conv.position_to_line_and_character(&script, byte_pos);
            assert_eq!(
                got_lc, expected_lc,
                "PositionToLineAndCharacter({}) mismatch in {name:?}",
                tup.byte_pos
            );

            let got_pos = conv.line_and_character_to_position(&script, expected_lc);
            assert_eq!(
                got_pos, byte_pos,
                "LineAndCharacterToPosition({},{}) mismatch in {name:?}",
                tup.line, tup.char
            );
        }
    }
}
