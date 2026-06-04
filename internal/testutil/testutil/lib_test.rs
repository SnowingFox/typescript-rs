use super::*;

#[test]
fn test_assert_panics_catches_expected_panic() {
    assert_panics(|| panic!("boom"), "boom");
}

#[test]
#[should_panic(expected = "did not panic")]
fn test_assert_panics_fails_when_no_panic() {
    assert_panics(|| { /* no panic */ }, "anything");
}

#[test]
#[should_panic(expected = "panic value mismatch")]
fn test_assert_panics_fails_on_wrong_value() {
    assert_panics(|| panic!("actual"), "expected");
}

#[test]
fn test_test_program_is_single_threaded_default() {
    // Without the env var set, the default should be !race::ENABLED, which
    // in default builds (no `race` feature) means `true`.
    std::env::remove_var("TS_TEST_PROGRAM_SINGLE_THREADED");
    let result = compute_test_program_is_single_threaded();
    assert_eq!(result, !tsgo_testutil_race::ENABLED);
}

#[test]
fn test_test_program_is_single_threaded_env_true() {
    std::env::set_var("TS_TEST_PROGRAM_SINGLE_THREADED", "true");
    let result = compute_test_program_is_single_threaded();
    assert!(result);
    std::env::remove_var("TS_TEST_PROGRAM_SINGLE_THREADED");
}

#[test]
fn test_test_program_is_single_threaded_env_false() {
    std::env::set_var("TS_TEST_PROGRAM_SINGLE_THREADED", "false");
    let result = compute_test_program_is_single_threaded();
    assert!(!result);
    std::env::remove_var("TS_TEST_PROGRAM_SINGLE_THREADED");
}
