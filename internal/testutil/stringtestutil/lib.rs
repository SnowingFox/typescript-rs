//! `tsgo_testutil_stringtestutil` — string helpers shared by the test suite.
//!
//! 1:1 port of Go `internal/testutil/stringtestutil` (`stringtestutil.go`).

use tsgo_stringutil::{guess_indentation, is_white_space_like};

/// Normalizes an indented multi-line template literal by stripping surrounding
/// blank lines and the common leading indentation, mirroring the helper used
/// throughout the Go test suite to author readable inline fixtures.
///
/// Leading tabs are expanded to 4 spaces before the common indentation is
/// measured (so tab- and space-indented fixtures dedent consistently). The
/// minimum indentation across non-blank lines is then removed from every line.
///
/// # Examples
/// ```
/// use tsgo_testutil_stringtestutil::dedent;
/// assert_eq!(dedent("\n    a\n    b\n"), "a\nb");
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/stringtestutil/stringtestutil.go:Dedent
pub fn dedent(text: &str) -> String {
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    // Remove blank lines in the beginning and end and convert all tabs in the
    // beginning of line to spaces.
    let mut start_line: isize = -1;
    let mut last_line: usize = 0;
    for (i, line) in lines.iter_mut().enumerate() {
        let first_non_white = line
            .char_indices()
            .find(|&(_, r)| !is_white_space_like(r))
            .map_or(-1, |(idx, _)| idx as isize);
        if first_non_white > 0 {
            let fnw = first_non_white as usize;
            let expanded = line[..fnw].replace('\t', "    ");
            line.replace_range(..fnw, &expanded);
        }
        if !line.trim().is_empty() {
            if start_line == -1 {
                start_line = i as isize;
            }
            last_line = i;
        }
    }
    let mut lines: Vec<String> = lines[start_line as usize..=last_line].to_vec();
    let mapped: Vec<&str> = lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                ""
            } else {
                line.as_str()
            }
        })
        .collect();
    let indentation = guess_indentation(&mapped);
    if indentation > 0 {
        for line in &mut lines {
            if line.len() > indentation {
                *line = line[indentation..].to_string();
            } else {
                *line = String::new();
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
