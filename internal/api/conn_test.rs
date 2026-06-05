use super::{unmarshal_params, ERR_CONN_CLOSED, ERR_REQUEST_TIMEOUT};

#[derive(serde::Deserialize, PartialEq, Debug)]
struct SampleParams {
    x: i32,
}

// Go: internal/api/conn.go:UnmarshalParams — empty params
#[test]
fn unmarshal_params_empty_returns_none() {
    let out: Option<SampleParams> = unmarshal_params(b"").unwrap();
    assert!(out.is_none());
}

// Go: internal/api/conn.go:UnmarshalParams — object params
#[test]
fn unmarshal_params_parses_object() {
    let out: SampleParams = unmarshal_params(br#"{"x":42}"#).unwrap().unwrap();
    assert_eq!(out.x, 42);
}

// Go: internal/api/conn.go:ErrConnClosed
#[test]
fn err_conn_closed_message() {
    assert_eq!(ERR_CONN_CLOSED, "api: connection closed");
}

// Go: internal/api/conn.go:ErrRequestTimeout
#[test]
fn err_request_timeout_message() {
    assert_eq!(ERR_REQUEST_TIMEOUT, "api: request timeout");
}
