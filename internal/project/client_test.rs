// Go: internal/project/client.go + extendedconfigcache_test.go:noopClient
use super::*;

#[test]
fn noop_client_is_active() {
    // Go: internal/project/extendedconfigcache_test.go:noopClient.IsActive
    let client = NoopClient;
    assert!(client.is_active());
}

#[test]
fn noop_client_watch_files_ok() {
    let client = NoopClient;
    let result = client.watch_files(WatcherID("w-1".to_string()), Vec::new());
    assert!(result.is_ok());
}

#[test]
fn noop_client_unwatch_files_ok() {
    let client = NoopClient;
    let result = client.unwatch_files(WatcherID("w-1".to_string()));
    assert!(result.is_ok());
}

#[test]
fn noop_client_refresh_diagnostics_ok() {
    let client = NoopClient;
    assert!(client.refresh_diagnostics().is_ok());
}

#[test]
fn noop_client_publish_diagnostics_ok() {
    let client = NoopClient;
    assert!(client.publish_diagnostics(serde_json::Value::Null).is_ok());
}

#[test]
fn noop_client_refresh_inlay_hints_ok() {
    let client = NoopClient;
    assert!(client.refresh_inlay_hints().is_ok());
}

#[test]
fn noop_client_refresh_code_lens_ok() {
    let client = NoopClient;
    assert!(client.refresh_code_lens().is_ok());
}

#[test]
fn noop_client_send_telemetry_ok() {
    let client = NoopClient;
    assert!(client.send_telemetry(serde_json::Value::Null).is_ok());
}

#[test]
fn watcher_id_equality() {
    // Go: internal/project/watch.go:WatcherID (string type)
    let a = WatcherID("w-1".to_string());
    let b = WatcherID("w-1".to_string());
    let c = WatcherID("w-2".to_string());
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn client_trait_is_object_safe() {
    fn _accept_dyn(_client: &dyn Client) {}
    let client = NoopClient;
    _accept_dyn(&client);
}
