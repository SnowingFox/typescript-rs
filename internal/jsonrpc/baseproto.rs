//! Base protocol for JSON-RPC with `Content-Length` headers (as used by LSP).
//!
//! See the LSP base protocol spec. [`Reader`] parses framed messages and
//! [`Writer`] emits them.

use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};

/// Errors produced by the base-protocol `Content-Length` framing.
#[derive(Debug, thiserror::Error)]
pub enum BaseProtoError {
    /// A header line had no `:` separator. The offending line is shown quoted,
    /// matching Go's `%q`.
    #[error("jsonrpc: invalid header: {0:?}")]
    InvalidHeader(String),
    /// The `Content-Length` value failed to parse as an integer.
    #[error("jsonrpc: invalid content length: parse error: {0}")]
    InvalidContentLengthParse(String),
    /// The `Content-Length` value was negative.
    #[error("jsonrpc: invalid content length: negative value {0}")]
    NegativeContentLength(i64),
    /// No usable `Content-Length` header was present (or it was `<= 0`).
    #[error("jsonrpc: no content length")]
    NoContentLength,
    /// Reading a header line failed (non-EOF I/O error).
    #[error("jsonrpc: read header: {0}")]
    ReadHeader(#[source] io::Error),
    /// Reading the message body failed (e.g. "unexpected EOF").
    #[error("jsonrpc: read content: {0}")]
    ReadContent(String),
}

/// Reads JSON-RPC messages with `Content-Length` framing.
pub struct Reader<R: Read> {
    r: BufReader<R>,
}

impl<R: Read> Reader<R> {
    /// Creates a new [`Reader`], buffering `r` internally.
    ///
    /// Side effects: none (no read happens until [`Reader::read`]).
    // Go: internal/jsonrpc/baseproto.go:NewReader
    pub fn new(r: R) -> Self {
        Reader {
            r: BufReader::new(r),
        }
    }

    /// Reads the next message payload, returning `Ok(None)` at a clean
    /// end-of-stream.
    ///
    /// Side effects: consumes bytes from the underlying reader.
    // Go: internal/jsonrpc/baseproto.go:(*Reader).Read
    pub fn read(&mut self) -> Result<Option<Vec<u8>>, BaseProtoError> {
        let mut content_length: i64 = 0;

        loop {
            let mut line = Vec::new();
            let n = self
                .r
                .read_until(b'\n', &mut line)
                .map_err(BaseProtoError::ReadHeader)?;
            // A header that ends at EOF (no trailing '\n', including the n == 0
            // case) is treated as a clean end-of-stream, mirroring Go's
            // `(nil, io.EOF)`.
            if n == 0 || !line.ends_with(b"\n") {
                return Ok(None);
            }
            if line == b"\r\n" {
                break;
            }

            match line.iter().position(|&b| b == b':') {
                None => {
                    return Err(BaseProtoError::InvalidHeader(
                        String::from_utf8_lossy(&line).into_owned(),
                    ));
                }
                Some(idx) => {
                    let key = &line[..idx];
                    let value = &line[idx + 1..];
                    if key == b"Content-Length" {
                        let trimmed = value.trim_ascii();
                        content_length =
                            String::from_utf8_lossy(trimmed)
                                .parse::<i64>()
                                .map_err(|e| {
                                    BaseProtoError::InvalidContentLengthParse(e.to_string())
                                })?;
                        if content_length < 0 {
                            return Err(BaseProtoError::NegativeContentLength(content_length));
                        }
                    }
                }
            }
        }

        if content_length <= 0 {
            return Err(BaseProtoError::NoContentLength);
        }

        let mut data = vec![0u8; content_length as usize];
        self.r.read_exact(&mut data).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                BaseProtoError::ReadContent("unexpected EOF".to_string())
            } else {
                BaseProtoError::ReadContent(e.to_string())
            }
        })?;

        Ok(Some(data))
    }
}

/// Writes JSON-RPC messages with `Content-Length` framing.
pub struct Writer<W: Write> {
    w: BufWriter<W>,
}

impl<W: Write> Writer<W> {
    /// Creates a new [`Writer`], buffering `w` internally.
    ///
    /// Side effects: none (no write happens until [`Writer::write`]).
    // Go: internal/jsonrpc/baseproto.go:NewWriter
    pub fn new(w: W) -> Self {
        Writer {
            w: BufWriter::new(w),
        }
    }

    /// Writes `data` framed with a `Content-Length` header, then flushes.
    ///
    /// Side effects: writes the header and body to the underlying writer and
    /// flushes it.
    // Go: internal/jsonrpc/baseproto.go:(*Writer).Write
    pub fn write(&mut self, data: &[u8]) -> io::Result<()> {
        write!(self.w, "Content-Length: {}\r\n\r\n", data.len())?;
        self.w.write_all(data)?;
        self.w.flush()
    }
}

#[cfg(test)]
#[path = "baseproto_test.rs"]
mod tests;
