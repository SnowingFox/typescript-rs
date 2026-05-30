use super::*;

use std::io::{self, Write};

// These cases mirror Go `lsp/lsproto/baseproto_test.go`, exercised through the
// `lsproto` wrappers (Go's `BaseReader`/`BaseWriter` embed `*jsonrpc.*`). Like
// the `tsgo_jsonrpc` port, a clean end-of-stream maps to `Ok(None)` instead of
// Go's `io.EOF`.

fn read_one(input: &[u8]) -> Result<Option<Vec<u8>>, tsgo_jsonrpc::BaseProtoError> {
    BaseReader::new(input).read()
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/empty
#[test]
fn base_reader_empty() {
    let err = read_one(b"Content-Length: 0\r\n\r\n").unwrap_err();
    assert_eq!(err.to_string(), "jsonrpc: no content length");
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/early end
#[test]
fn base_reader_early_end() {
    assert!(matches!(read_one(b"oops"), Ok(None)));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/negative length
#[test]
fn base_reader_negative_length() {
    let err = read_one(b"Content-Length: -1\r\n\r\n").unwrap_err();
    assert_eq!(
        err.to_string(),
        "jsonrpc: invalid content length: negative value -1"
    );
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/invalid content
#[test]
fn base_reader_invalid_content() {
    let got = read_one(b"Content-Length: 1\r\n\r\n{").unwrap();
    assert_eq!(got, Some(b"{".to_vec()));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/valid content
#[test]
fn base_reader_valid_content() {
    let got = read_one(b"Content-Length: 2\r\n\r\n{}").unwrap();
    assert_eq!(got, Some(b"{}".to_vec()));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/extra header values
#[test]
fn base_reader_extra_header_values() {
    let got = read_one(b"Content-Length: 2\r\nExtra: 1\r\n\r\n{}").unwrap();
    assert_eq!(got, Some(b"{}".to_vec()));
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/too long content length
#[test]
fn base_reader_too_long_content_length() {
    let err = read_one(b"Content-Length: 100\r\n\r\n{}").unwrap_err();
    assert_eq!(err.to_string(), "jsonrpc: read content: unexpected EOF");
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/missing content length
#[test]
fn base_reader_missing_content_length() {
    let err = read_one(b"Content-Length: \r\n\r\n{}").unwrap_err();
    assert!(
        err.to_string()
            .starts_with("jsonrpc: invalid content length: parse error:"),
        "got {err}"
    );
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReader/invalid header
#[test]
fn base_reader_invalid_header() {
    let err = read_one(b"Nope\r\n\r\n{}").unwrap_err();
    assert_eq!(err.to_string(), "jsonrpc: invalid header: \"Nope\\r\\n\"");
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseReaderMultipleReads
#[test]
fn base_reader_multiple_reads() {
    let stream = b"Content-Length: 4\r\n\r\n1234Content-Length: 2\r\n\r\n{}";
    let mut reader = BaseReader::new(&stream[..]);
    assert_eq!(reader.read().unwrap(), Some(b"1234".to_vec()));
    assert_eq!(reader.read().unwrap(), Some(b"{}".to_vec()));
    assert!(matches!(reader.read(), Ok(None)));
}

fn write_one(data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = BaseWriter::new(&mut buf);
        w.write(data).unwrap();
    }
    buf
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseWriter/empty
#[test]
fn base_writer_empty() {
    assert_eq!(write_one(b"{}"), b"Content-Length: 2\r\n\r\n{}".to_vec());
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseWriter/bigger object
#[test]
fn base_writer_bigger_object() {
    let expected = b"Content-Length: 15\r\n\r\n{\"key\":\"value\"}";
    assert_eq!(write_one(br#"{"key":"value"}"#), expected.to_vec());
}

// Go: lsp/lsproto/baseproto_test.go:TestBaseWriterWriteError
#[test]
fn base_writer_write_error() {
    struct ErrorWriter;
    impl Write for ErrorWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("test error"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("test error"))
        }
    }

    let mut w = BaseWriter::new(ErrorWriter);
    let err = w.write(b"{}").unwrap_err();
    assert_eq!(err.to_string(), "test error");
}
