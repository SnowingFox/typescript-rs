//! Port of Go's `internal/execute/tsc/statistics.go`.
//!
//! The [`Statistics`] struct accumulates file counts, identifier counts,
//! type-checking counters, memory usage, and compile times. It can be reported
//! to a writer (for `--diagnostics`/`--extendedDiagnostics`) and aggregated
//! across projects for `--build` mode.

use std::io::Write;
use std::time::Duration;

/// Compile-time breakdown counters (config parse, build-info read, parse, bind,
/// check, emit, total).
///
/// Side effects: none (plain data).
// Go: internal/execute/tsc/compile.go:CompileTimes
#[derive(Debug, Default, Clone)]
pub struct CompileTimes {
    pub config_time: Duration,
    pub build_info_read_time: Duration,
    pub parse_time: Duration,
    pub bind_time: Duration,
    pub check_time: Duration,
    pub emit_time: Duration,
    pub changes_compute_time: Duration,
    pub total_time: Duration,
}

/// A single row in the statistics table.
#[derive(Debug)]
struct TableRow {
    name: String,
    value: String,
}

/// A statistics table that can be printed to a writer.
#[derive(Debug, Default)]
struct Table {
    rows: Vec<TableRow>,
}

impl Table {
    fn add(&mut self, name: &str, value: impl std::fmt::Display) {
        self.rows.push(TableRow {
            name: name.to_string(),
            value: format!("{value}"),
        });
    }

    fn add_duration(&mut self, name: &str, d: Duration) {
        self.add(name, format_duration(d));
    }

    fn print(&self, w: &mut dyn Write) {
        let name_width = self.rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
        let value_width = self.rows.iter().map(|r| r.value.len()).max().unwrap_or(0);
        for row in &self.rows {
            let _ = writeln!(
                w,
                "{:<nw$} {:>vw$}",
                format!("{}:", row.name),
                row.value,
                nw = name_width + 1,
                vw = value_width,
            );
        }
    }
}

/// Formats a [`Duration`] as `"X.XXXs"`.
// Go: internal/execute/tsc/statistics.go:formatDuration
fn format_duration(d: Duration) -> String {
    format!("{:.3}s", d.as_secs_f64())
}

/// Accumulated statistics for a compilation run (or an aggregate of runs).
///
/// Side effects: none (plain data).
// Go: internal/execute/tsc/statistics.go:Statistics
#[derive(Debug, Default, Clone)]
pub struct Statistics {
    pub is_aggregate: bool,
    pub projects: usize,
    pub projects_built: usize,
    pub timestamp_updates: usize,
    pub files: usize,
    pub lines: usize,
    pub identifiers: usize,
    pub symbols: usize,
    pub types: usize,
    pub instantiations: usize,
    pub memory_used: u64,
    pub memory_allocs: u64,
    pub compile_times: CompileTimes,
}

impl Statistics {
    /// Reports the statistics as a formatted table to `w`.
    ///
    /// Mirrors Go's `Statistics.Report`.
    ///
    /// Side effects: writes the formatted table to `w`.
    // Go: internal/execute/tsc/statistics.go:Statistics.Report
    pub fn report(&self, w: &mut dyn Write) {
        let mut table = Table::default();
        let prefix = if self.is_aggregate {
            table.add("Projects in scope", self.projects);
            table.add("Projects built", self.projects_built);
            table.add("Timestamps only updates", self.timestamp_updates);
            "Aggregate "
        } else {
            ""
        };
        table.add(&format!("{prefix}Files"), self.files);
        table.add(&format!("{prefix}Lines"), self.lines);
        table.add(&format!("{prefix}Identifiers"), self.identifiers);
        table.add(&format!("{prefix}Symbols"), self.symbols);
        table.add(&format!("{prefix}Types"), self.types);
        table.add(&format!("{prefix}Instantiations"), self.instantiations);
        table.add(
            &format!("{prefix}Memory used"),
            format!("{}K", self.memory_used / 1024),
        );
        table.add(
            &format!("{prefix}Memory allocs"),
            self.memory_allocs.to_string(),
        );
        if self.compile_times.config_time != Duration::ZERO {
            table.add_duration(
                &format!("{prefix}Config time"),
                self.compile_times.config_time,
            );
        }
        if self.compile_times.build_info_read_time != Duration::ZERO {
            table.add_duration(
                &format!("{prefix}BuildInfo read time"),
                self.compile_times.build_info_read_time,
            );
        }
        table.add_duration(
            &format!("{prefix}Parse time"),
            self.compile_times.parse_time,
        );
        if self.compile_times.bind_time != Duration::ZERO {
            table.add_duration(&format!("{prefix}Bind time"), self.compile_times.bind_time);
        }
        if self.compile_times.check_time != Duration::ZERO {
            table.add_duration(
                &format!("{prefix}Check time"),
                self.compile_times.check_time,
            );
        }
        if self.compile_times.emit_time != Duration::ZERO {
            table.add_duration(&format!("{prefix}Emit time"), self.compile_times.emit_time);
        }
        if self.compile_times.changes_compute_time != Duration::ZERO {
            table.add_duration(
                &format!("{prefix}Changes compute time"),
                self.compile_times.changes_compute_time,
            );
        }
        table.add_duration(
            &format!("{prefix}Total time"),
            self.compile_times.total_time,
        );
        table.print(w);
    }

    /// Aggregates another `Statistics` into this one.
    ///
    /// Side effects: mutates `self`.
    // Go: internal/execute/tsc/statistics.go:Statistics.Aggregate
    pub fn aggregate(&mut self, other: &Statistics) {
        self.is_aggregate = true;
        self.files += other.files;
        self.lines += other.lines;
        self.identifiers += other.identifiers;
        self.symbols += other.symbols;
        self.types += other.types;
        self.instantiations += other.instantiations;
        self.memory_used += other.memory_used;
        self.memory_allocs += other.memory_allocs;
        self.compile_times.config_time += other.compile_times.config_time;
        self.compile_times.build_info_read_time += other.compile_times.build_info_read_time;
        self.compile_times.parse_time += other.compile_times.parse_time;
        self.compile_times.bind_time += other.compile_times.bind_time;
        self.compile_times.check_time += other.compile_times.check_time;
        self.compile_times.emit_time += other.compile_times.emit_time;
        self.compile_times.changes_compute_time += other.compile_times.changes_compute_time;
    }

    /// Sets the total compilation time.
    // Go: internal/execute/tsc/statistics.go:Statistics.SetTotalTime
    pub fn set_total_time(&mut self, total: Duration) {
        self.compile_times.total_time = total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_three_decimal_places() {
        let d = Duration::from_millis(1234);
        assert_eq!(format_duration(d), "1.234s");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(Duration::ZERO), "0.000s");
    }

    #[test]
    fn statistics_default_is_zeroed() {
        let s = Statistics::default();
        assert_eq!(s.files, 0);
        assert!(!s.is_aggregate);
    }

    #[test]
    fn statistics_aggregate_sums_values() {
        let mut agg = Statistics::default();
        let s1 = Statistics {
            files: 10,
            lines: 500,
            identifiers: 200,
            symbols: 100,
            types: 50,
            instantiations: 25,
            memory_used: 1024 * 100,
            memory_allocs: 5000,
            ..Default::default()
        };
        let s2 = Statistics {
            files: 5,
            lines: 250,
            identifiers: 100,
            symbols: 50,
            types: 25,
            instantiations: 10,
            memory_used: 1024 * 50,
            memory_allocs: 2500,
            ..Default::default()
        };
        agg.aggregate(&s1);
        agg.aggregate(&s2);
        assert!(agg.is_aggregate);
        assert_eq!(agg.files, 15);
        assert_eq!(agg.lines, 750);
        assert_eq!(agg.identifiers, 300);
        assert_eq!(agg.symbols, 150);
        assert_eq!(agg.types, 75);
        assert_eq!(agg.instantiations, 35);
        assert_eq!(agg.memory_used, 1024 * 150);
        assert_eq!(agg.memory_allocs, 7500);
    }

    #[test]
    fn statistics_report_produces_output() {
        let s = Statistics {
            files: 42,
            lines: 1000,
            identifiers: 500,
            symbols: 250,
            types: 100,
            instantiations: 50,
            memory_used: 2048 * 1024,
            memory_allocs: 10000,
            compile_times: CompileTimes {
                parse_time: Duration::from_millis(123),
                total_time: Duration::from_millis(456),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = Vec::new();
        s.report(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Files:"));
        assert!(output.contains("42"));
        assert!(output.contains("Parse time:"));
        assert!(output.contains("0.123s"));
        assert!(output.contains("Total time:"));
        assert!(output.contains("0.456s"));
        assert!(!output.contains("Projects in scope"));
    }

    #[test]
    fn aggregate_statistics_report_shows_project_lines() {
        let mut s = Statistics::default();
        s.is_aggregate = true;
        s.projects = 3;
        s.projects_built = 2;
        s.timestamp_updates = 1;
        s.compile_times.total_time = Duration::from_secs(1);
        let mut buf = Vec::new();
        s.report(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Projects in scope:"));
        assert!(output.contains("Projects built:"));
        assert!(output.contains("Timestamps only updates:"));
        assert!(output.contains("Aggregate Files:"));
    }

    #[test]
    fn set_total_time() {
        let mut s = Statistics::default();
        s.set_total_time(Duration::from_secs(5));
        assert_eq!(s.compile_times.total_time, Duration::from_secs(5));
    }
}
