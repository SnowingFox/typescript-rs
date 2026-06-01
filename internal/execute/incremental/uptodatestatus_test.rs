use super::*;
use crate::{BuildInfo, BuildInfoFileInfo};
use std::time::{Duration, UNIX_EPOCH};

// b.ts content + its real version hash (from `cmd/tsgo`).
const B_TS: &str = "export const b = 1;\n";
const B_TS_VERSION: &str = "90312e1cbc42534115cfa9601aa41950";

fn build_info_for_b(version: &str) -> BuildInfo {
    BuildInfo {
        version: tsgo_core::version::version().to_string(),
        file_names: vec!["./b.ts".to_string()],
        file_infos: vec![BuildInfoFileInfo::signature(version)],
        ..BuildInfo::default()
    }
}

fn at(secs: u64) -> std::time::SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn input(file_name: &str, mtime_secs: Option<u64>, text: Option<&str>) -> InputFile {
    InputFile {
        file_name: file_name.to_string(),
        mtime: mtime_secs.map(at),
        current_text: text.map(str::to_string),
    }
}

// Go: internal/execute/build/buildtask.go:getUpToDateStatus
// (buildInfo == nil -> upToDateStatusTypeOutputMissing)
#[test]
fn missing_tsbuildinfo_is_out_of_date() {
    let inputs = [input("./b.ts", Some(100), Some(B_TS))];
    let status = get_up_to_date_status(&inputs, None, at(200));
    assert_eq!(status, UpToDateStatusType::OutputMissing);
    assert!(!status.is_up_to_date());
}

// Go: getUpToDateStatus (input newer + version mismatch -> upToDateStatusTypeInputFileNewer)
#[test]
fn input_newer_with_changed_text_is_out_of_date() {
    let bi = build_info_for_b(B_TS_VERSION);
    // input mtime (300) is after the buildinfo mtime (200); text changed.
    let inputs = [input("./b.ts", Some(300), Some("export const b = 2;\n"))];
    let status = get_up_to_date_status(&inputs, Some(&bi), at(200));
    assert_eq!(status, UpToDateStatusType::InputFileNewer);
    assert!(!status.is_up_to_date());
}

// Go: getUpToDateStatus (input newer but version unchanged -> UpToDateWithInputFileText)
#[test]
fn input_newer_with_unchanged_text_is_up_to_date() {
    let bi = build_info_for_b(B_TS_VERSION);
    let inputs = [input("./b.ts", Some(300), Some(B_TS))];
    let status = get_up_to_date_status(&inputs, Some(&bi), at(200));
    assert_eq!(status, UpToDateStatusType::UpToDateWithInputFileText);
    assert!(status.is_up_to_date());
}

// Go: getUpToDateStatus (all inputs older than buildinfo -> upToDateStatusTypeUpToDate)
#[test]
fn all_inputs_older_than_buildinfo_is_up_to_date() {
    let bi = build_info_for_b(B_TS_VERSION);
    let inputs = [input("./b.ts", Some(100), Some(B_TS))];
    let status = get_up_to_date_status(&inputs, Some(&bi), at(200));
    assert_eq!(status, UpToDateStatusType::UpToDate);
    assert!(status.is_up_to_date());
}

// Go: getUpToDateStatus (inputTime.IsZero() -> upToDateStatusTypeInputFileMissing)
#[test]
fn missing_input_file_is_out_of_date() {
    let bi = build_info_for_b(B_TS_VERSION);
    let inputs = [input("./b.ts", None, None)];
    let status = get_up_to_date_status(&inputs, Some(&bi), at(200));
    assert_eq!(status, UpToDateStatusType::InputFileMissing);
    assert!(!status.is_up_to_date());
}

// Go: getUpToDateStatus (!buildInfo.IsValidVersion() -> upToDateStatusTypeTsVersionOutputOfDate)
#[test]
fn version_mismatch_is_out_of_date() {
    let mut bi = build_info_for_b(B_TS_VERSION);
    bi.version = "0.0.0-old".to_string();
    let inputs = [input("./b.ts", Some(100), Some(B_TS))];
    let status = get_up_to_date_status(&inputs, Some(&bi), at(200));
    assert_eq!(status, UpToDateStatusType::TsVersionOutputOfDate);
    assert!(!status.is_up_to_date());
}
