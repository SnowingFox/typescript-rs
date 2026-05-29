use super::*;

// Go side `internal/jsonrpc` has no `*_test.go`; the base-protocol framing is
// exercised by `lsp/lsproto/baseproto_test.go` (via thin wrappers over these
// very types). Those sub-cases are the ground truth and are replicated here,
// with expected values taken from the Go test literals.

fn read_one(input: &[u8]) -> Result<Option<Vec<u8>>, BaseProtoError> {
    Reader::new(input).read()
}

// --- Reader: TestBaseReader sub-cases ---

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/empty
#[test]
fn read_empty_zero_length() {
    let err = read_one(b"Content-Length: 0\r\n\r\n").unwrap_err();
    assert_eq!(err.to_string(), "jsonrpc: no content length");
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/early end
#[test]
fn read_early_end() {
    // Go returns io.EOF here; this port maps a clean EOF to Ok(None).
    assert!(matches!(read_one(b"oops"), Ok(None)));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/negative length
#[test]
fn read_negative_length() {
    let err = read_one(b"Content-Length: -1\r\n\r\n").unwrap_err();
    assert_eq!(
        err.to_string(),
        "jsonrpc: invalid content length: negative value -1"
    );
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/invalid content
#[test]
fn read_invalid_content_one_byte() {
    let got = read_one(b"Content-Length: 1\r\n\r\n{").unwrap();
    assert_eq!(got, Some(b"{".to_vec()));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/valid content
#[test]
fn read_valid_content() {
    let got = read_one(b"Content-Length: 2\r\n\r\n{}").unwrap();
    assert_eq!(got, Some(b"{}".to_vec()));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/extra header values
#[test]
fn read_extra_header_values() {
    let got = read_one(b"Content-Length: 2\r\nExtra: 1\r\n\r\n{}").unwrap();
    assert_eq!(got, Some(b"{}".to_vec()));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/too long content length
#[test]
fn read_too_long_content_length() {
    let err = read_one(b"Content-Length: 100\r\n\r\n{}").unwrap_err();
    assert_eq!(err.to_string(), "jsonrpc: read content: unexpected EOF");
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/missing content length
#[test]
fn read_missing_content_length_value() {
    let err = read_one(b"Content-Length: \r\n\r\n{}").unwrap_err();
    // Rust's integer-parse text differs from Go's strconv "invalid syntax";
    // the framing wrapper prefix is preserved verbatim.
    assert!(
        err.to_string()
            .starts_with("jsonrpc: invalid content length: parse error:"),
        "got {err}"
    );
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/invalid header
#[test]
fn read_invalid_header() {
    let err = read_one(b"Nope\r\n\r\n{}").unwrap_err();
    assert_eq!(err.to_string(), "jsonrpc: invalid header: \"Nope\\r\\n\"");
}

// --- Reader: TestBaseReaderMultipleReads ---

// Go: lsp/lsproto/baseproto_test.go:TestBaseReaderMultipleReads
#[test]
fn read_multiple_messages() {
    let stream = b"Content-Length: 4\r\n\r\n1234Content-Length: 2\r\n\r\n{}";
    let mut reader = Reader::new(&stream[..]);
    assert_eq!(reader.read().unwrap(), Some(b"1234".to_vec()));
    assert_eq!(reader.read().unwrap(), Some(b"{}".to_vec()));
    assert!(matches!(reader.read(), Ok(None)));
}

// --- Writer: TestBaseWriter / TestBaseWriterWriteError ---

fn write_one(data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = Writer::new(&mut buf);
        w.write(data).unwrap();
    }
    buf
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseWriter/empty
#[test]
fn write_empty_object() {
    assert_eq!(write_one(b"{}"), b"Content-Length: 2\r\n\r\n{}".to_vec());
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseWriter/bigger object
#[test]
fn write_bigger_object() {
    let expected = b"Content-Length: 15\r\n\r\n{\"key\":\"value\"}";
    assert_eq!(write_one(br#"{"key":"value"}"#), expected.to_vec());
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseWriterWriteError
#[test]
fn write_propagates_io_error() {
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("test error"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("test error"))
        }
    }

    let mut w = Writer::new(FailingWriter);
    let err = w.write(b"{}").unwrap_err();
    assert_eq!(err.to_string(), "test error");
}

// --- Round-trip: encode then decode a spec-known payload ---

// Go: internal/jsonrpc/baseproto.go:(*Writer).Write + (*Reader).Read
#[test]
fn framing_roundtrip() {
    let payload = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
    let framed = write_one(payload);
    let got = Reader::new(framed.as_slice()).read().unwrap();
    assert_eq!(got, Some(payload.to_vec()));
}
