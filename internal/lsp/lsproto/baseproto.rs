//! Thin LSP base-protocol wrappers (`Content-Length` framing).
//!
//! Go embeds `*jsonrpc.Reader`/`*jsonrpc.Writer`; this port wraps the already
//! ported `tsgo_jsonrpc::Reader`/`Writer` for backwards compatibility.

use std::io::{Read, Write};

use tsgo_jsonrpc::{BaseProtoError, Reader, Writer};

/// Wraps [`tsgo_jsonrpc::Reader`] for backwards compatibility, mirroring Go's
/// `BaseReader` (which embeds `*jsonrpc.Reader`).
///
/// # Examples
/// ```
/// let mut r = tsgo_lsproto::BaseReader::new(&b"Content-Length: 2\r\n\r\n{}"[..]);
/// assert_eq!(r.read().unwrap(), Some(b"{}".to_vec()));
/// ```
// Go: internal/lsp/lsproto/baseproto.go:BaseReader
pub struct BaseReader<R: Read> {
    inner: Reader<R>,
}

impl<R: Read> BaseReader<R> {
    /// Creates a new [`BaseReader`] wrapping `r`.
    ///
    /// Side effects: none (no read happens until [`BaseReader::read`]).
    // Go: internal/lsp/lsproto/baseproto.go:NewBaseReader
    pub fn new(r: R) -> Self {
        BaseReader {
            inner: Reader::new(r),
        }
    }

    /// Reads the next framed message payload, returning `Ok(None)` at a clean
    /// end-of-stream (Go returns `io.EOF`).
    ///
    /// Side effects: consumes bytes from the underlying reader.
    // Go: internal/jsonrpc/baseproto.go:(*Reader).Read
    pub fn read(&mut self) -> Result<Option<Vec<u8>>, BaseProtoError> {
        self.inner.read()
    }
}

/// Wraps [`tsgo_jsonrpc::Writer`] for backwards compatibility, mirroring Go's
/// `BaseWriter` (which embeds `*jsonrpc.Writer`).
///
/// # Examples
/// ```
/// let mut buf = Vec::new();
/// tsgo_lsproto::BaseWriter::new(&mut buf).write(b"{}").unwrap();
/// assert_eq!(buf, b"Content-Length: 2\r\n\r\n{}".to_vec());
/// ```
// Go: internal/lsp/lsproto/baseproto.go:BaseWriter
pub struct BaseWriter<W: Write> {
    inner: Writer<W>,
}

impl<W: Write> BaseWriter<W> {
    /// Creates a new [`BaseWriter`] wrapping `w`.
    ///
    /// Side effects: none (no write happens until [`BaseWriter::write`]).
    // Go: internal/lsp/lsproto/baseproto.go:NewBaseWriter
    pub fn new(w: W) -> Self {
        BaseWriter {
            inner: Writer::new(w),
        }
    }

    /// Writes `data` framed with a `Content-Length` header, then flushes.
    ///
    /// Side effects: writes the header and body to the underlying writer.
    // Go: internal/jsonrpc/baseproto.go:(*Writer).Write
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.inner.write(data)
    }
}

#[cfg(test)]
#[path = "baseproto_test.rs"]
mod tests;
