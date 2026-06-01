//! The `--build` orchestrator: graph generation, the topological build loop,
//! per-project up-to-date checking, and reporting.
//!
//! Ports the reachable subset of Go `internal/execute/build/orchestrator.go`
//! (`Orchestrator`/`Start`/`buildOrClean`), `buildtask.go`
//! (`buildProject`/`getUpToDateStatus`/`compileAndEmit`), and
//! `uptodatestatus.go` (the status kinds). The orchestrator is single-threaded
//! here (Go fans the build loop out over a `core.WorkGroup`); building in
//! topological order keeps the reported output identical to Go's
//! report-in-order behaviour, so no per-task output buffering is needed.
//!
//! ## Deferred (blocked-by, later P9 chunks)
//!
//! - `--watch -b` (downstream tracking, the watch loop). blocked-by: p9-watcher.
//! - `--clean` (output deletion). blocked-by: this chunk's `--clean` path.
//! - Pseudo-builds / `UpToDateWithUpstreamTypes` (touch-only timestamp updates,
//!   `.d.ts`-unchanged fast rebuilds) and the incremental skip-emit reuse.
//!   blocked-by: more of P6-9b (`HasChangedDtsFile`, pending-emit reuse).
//! - Parallel builders (`--builders`) and `--stopBuildOnErrors` upstream-error
//!   propagation beyond the reachable subset.
//! - Pretty (colour) `--build` status lines. blocked-by: the colour styling the
//!   p9-watcher chunk also needs.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tsgo_compiler::{
    get_resolved_project_reference, new_compiler_host, new_program, resolve_project_references,
    CompilerHost, ProgramOptions, ProjectReferenceDiagnostic,
};
use tsgo_core::projectreference::resolve_config_file_name_of_project_reference;
use tsgo_diagnostics::Message;
use tsgo_diagnosticwriter::{
    format_diagnostics_status_and_time, format_diagnostics_status_with_color_and_time,
    FormattingOptions,
};
use tsgo_incremental::{
    get_up_to_date_status, read_build_info, InputFile, Program as IncrementalProgram,
    UpToDateStatusType,
};
use tsgo_outputpaths::get_build_info_file_name;
use tsgo_tsoptions::{ParsedBuildCommandLine, ParsedCommandLine};
use tsgo_tspath::{convert_to_relative_path, resolve_path, ComparePathsOptions, Path};

use crate::sys::System;
use crate::tsc::{
    create_diagnostic_reporter, create_report_error_summary, emit_and_report_statistics,
    CommandLineResult, DiagnosticReporter, ExitStatus, ReportErrorSummary, ReportedDiagnostic,
};

/// Runs `tsc -b`: reports up-front command-line errors, otherwise resolves the
/// project-reference graph for the requested projects and builds each project in
/// topological order (skipping the up-to-date ones).
///
/// Mirrors Go's `tscBuildCompilation`. The `--help`/`--version`/pprof/`--watch`
/// branches are DEFER; this entry runs the non-watch build path.
///
/// Side effects: reads project configs/inputs and emits each built project's
/// outputs + `.tsbuildinfo` through `sys`'s file system; writes diagnostics and
/// status lines to `sys`.
// Go: internal/execute/tsc.go:tscBuildCompilation
pub fn perform_build(sys: &dyn System, build_command: ParsedBuildCommandLine) -> CommandLineResult {
    let locale = tsgo_locale::parse("en").expect("en locale is always available");
    let report_diagnostic =
        create_diagnostic_reporter(sys, &locale, &build_command.compiler_options);

    if !build_command.errors.is_empty() {
        for error in &build_command.errors {
            report_diagnostic.report(sys, &ReportedDiagnostic::from_parser(error, None, &locale));
        }
        return CommandLineResult {
            status: ExitStatus::DiagnosticsPresentOutputsSkipped,
        };
    }

    let orchestrator = Orchestrator::new(sys, &build_command);
    orchestrator.start()
}

/// The reachable subset of Go's `upToDateStatus`: the build-relevant outcome of
/// checking a project against its `.tsbuildinfo` and inputs, carrying the file
/// names the verbose reporter needs.
// Go: internal/execute/build/uptodatestatus.go:upToDateStatus
enum BuildStatus {
    /// The config file could not be resolved (Go `ConfigFileNotFound`).
    ConfigFileNotFound,
    /// A solution file (no inputs of its own, only references) — nothing to
    /// build (Go `Solution`).
    Solution,
    /// `--force` was given: rebuild unconditionally (Go `ForceBuild`).
    ForceBuild,
    /// Everything is current; no build needed (Go `UpToDate` /
    /// `UpToDateWithInputFileText`).
    UpToDate {
        newest_input: String,
        output: String,
    },
    /// The `.tsbuildinfo` (or another output) is missing (Go `OutputMissing`).
    OutputMissing { output: String },
    /// An input file is missing (Go `InputFileMissing`).
    InputFileMissing { input: String },
    /// An input file is newer than the output and its text changed (Go
    /// `InputFileNewer`).
    InputFileNewer { input: String, output: String },
    /// The `.tsbuildinfo` was written by a different compiler version (Go
    /// `TsVersionOutputOfDate`); carries the stale version string.
    TsVersionOutputOfDate { build_version: String },
}

/// The resolved project-reference graph for the requested projects: the
/// topological build order (config names), each project's parsed config keyed by
/// canonical path, and any `TS6202` circular-graph diagnostics.
// Go: internal/execute/build/orchestrator.go:Orchestrator (order/tasks/errors)
struct Graph {
    order: Vec<String>,
    configs: HashMap<Path, Option<ParsedCommandLine>>,
    circular: Vec<ReportedDiagnostic>,
}

/// The `--build` orchestrator over a [`System`].
// Go: internal/execute/build/orchestrator.go:Orchestrator
struct Orchestrator<'a> {
    sys: &'a dyn System,
    command: &'a ParsedBuildCommandLine,
    locale: tsgo_locale::Locale,
    host: Arc<dyn CompilerHost>,
    compare_paths_options: ComparePathsOptions,
    report_diagnostic: DiagnosticReporter,
}

impl<'a> Orchestrator<'a> {
    // Go: internal/execute/build/orchestrator.go:NewOrchestrator
    fn new(sys: &'a dyn System, command: &'a ParsedBuildCommandLine) -> Orchestrator<'a> {
        let locale = tsgo_locale::parse("en").expect("en locale is always available");
        let compare_paths_options = ComparePathsOptions {
            current_directory: sys.get_current_directory().to_string(),
            use_case_sensitive_file_names: sys.fs().use_case_sensitive_file_names(),
        };
        let host: Arc<dyn CompilerHost> = Arc::new(new_compiler_host(
            sys.get_current_directory().to_string(),
            sys.fs(),
            sys.default_library_path().to_string(),
        ));
        let report_diagnostic = create_diagnostic_reporter(sys, &locale, &command.compiler_options);
        Orchestrator {
            sys,
            command,
            locale,
            host,
            compare_paths_options,
            report_diagnostic,
        }
    }

    /// Generates the graph and drives the build loop.
    // Go: internal/execute/build/orchestrator.go:Orchestrator.Start
    fn start(&self) -> CommandLineResult {
        let graph = self.generate_graph();
        self.build_or_clean(graph)
    }

    /// The resolved config file names of the requested projects (Go's
    /// `ParsedBuildCommandLine.ResolvedProjectPaths`): each project is resolved
    /// against the current directory and given `tsconfig.json` if it is a
    /// directory.
    // Go: internal/tsoptions/parsedbuildcommandline.go:ParsedBuildCommandLine.ResolvedProjectPaths
    fn resolved_project_paths(&self) -> Vec<String> {
        self.command
            .projects
            .iter()
            .map(|project| {
                let absolute =
                    resolve_path(&self.compare_paths_options.current_directory, &[project]);
                resolve_config_file_name_of_project_reference(&absolute)
            })
            .collect()
    }

    /// Resolves the project-reference graph for every requested project and
    /// merges their topological build orders (deduped by canonical path).
    ///
    /// Mirrors Go's `GenerateGraph` + `setupBuildTask`: a project that could not
    /// be resolved still occupies a slot in the order (so its
    /// config-file-not-found error is reported in place).
    // Go: internal/execute/build/orchestrator.go:Orchestrator.GenerateGraph
    fn generate_graph(&self) -> Graph {
        let mut order: Vec<String> = Vec::new();
        let mut seen: HashSet<Path> = HashSet::new();
        let mut configs: HashMap<Path, Option<ParsedCommandLine>> = HashMap::new();
        let mut circular: Vec<ReportedDiagnostic> = Vec::new();

        for project in self.resolved_project_paths() {
            let root_path = self.to_path(&project);
            match get_resolved_project_reference(self.host.as_ref(), &project) {
                None => {
                    if seen.insert(root_path.clone()) {
                        configs.insert(root_path, None);
                        order.push(project);
                    }
                }
                Some(root_config) => {
                    let graph =
                        resolve_project_references(self.host.as_ref(), &project, &root_config);
                    let build_order = graph.get_build_order();
                    for diagnostic in &build_order.circular_diagnostics {
                        circular.push(self.project_ref_diagnostic(diagnostic));
                    }
                    for config_name in build_order.order {
                        let path = graph.to_path(&config_name);
                        if seen.insert(path.clone()) {
                            configs.insert(
                                path.clone(),
                                graph.get_resolved_reference_for(&path).cloned(),
                            );
                            order.push(config_name);
                        }
                    }
                }
            }
        }

        Graph {
            order,
            configs,
            circular,
        }
    }

    /// Builds each project in topological order, returning the worst per-project
    /// exit status. Circular-graph diagnostics short-circuit the whole build.
    // Go: internal/execute/build/orchestrator.go:Orchestrator.buildOrClean
    fn build_or_clean(&self, graph: Graph) -> CommandLineResult {
        let mut all_diagnostics: Vec<ReportedDiagnostic> = Vec::new();
        let status;

        if graph.circular.is_empty() {
            if !self.command.build_options.clean.is_true()
                && self.command.build_options.verbose.is_true()
            {
                let arg: String = graph
                    .order
                    .iter()
                    .map(|p| format!("\r\n    * {}", self.relative_file_name(p)))
                    .collect();
                self.report_builder_status(
                    &tsgo_diagnostics::PROJECTS_IN_THIS_BUILD_COLON_0,
                    vec![arg],
                );
            }

            let mut worst = ExitStatus::Success;
            for config_name in &graph.order {
                let project_status =
                    self.build_project(config_name, &graph.configs, &mut all_diagnostics);
                if (project_status as i32) > (worst as i32) {
                    worst = project_status;
                }
            }
            status = worst;
        } else {
            // Circularity errors prevent any project from being built.
            for diagnostic in &graph.circular {
                self.report_diagnostic.report(self.sys, diagnostic);
            }
            all_diagnostics = graph.circular;
            status = ExitStatus::ProjectReferenceCycleOutputsSkipped;
        }

        // Final error summary (a no-op in plain mode, matching Go).
        let summary =
            create_report_error_summary(self.sys, &self.locale, &self.command.compiler_options);
        summary.report(self.sys, &all_diagnostics);

        CommandLineResult { status }
    }

    /// Builds one project: compute its up-to-date status, report it (verbose),
    /// and either skip it or build + emit it.
    // Go: internal/execute/build/buildtask.go:BuildTask.buildProject
    fn build_project(
        &self,
        config_name: &str,
        configs: &HashMap<Path, Option<ParsedCommandLine>>,
        all_diagnostics: &mut Vec<ReportedDiagnostic>,
    ) -> ExitStatus {
        let path = self.to_path(config_name);
        let resolved = configs.get(&path).cloned().flatten();
        let status = self.get_up_to_date_status(&resolved);
        self.report_up_to_date_status(config_name, &status);
        if let Some(exit) =
            self.handle_status_that_doesnt_require_build(config_name, &status, all_diagnostics)
        {
            return exit;
        }
        let config = resolved.expect("a needs-build status implies a resolved config");
        self.compile_and_emit(config_name, &config, all_diagnostics)
    }

    /// Decides a project's up-to-date status from its `.tsbuildinfo` and inputs,
    /// reusing the incremental up-to-date predicate and augmenting it with the
    /// file names the verbose reporter needs.
    ///
    /// DEFER(P9): the upstream-error / pseudo-build / output-timestamp branches
    /// of Go's `getUpToDateStatus`. blocked-by: pseudo-builds + watch.
    // Go: internal/execute/build/buildtask.go:BuildTask.getUpToDateStatus
    fn get_up_to_date_status(&self, resolved: &Option<ParsedCommandLine>) -> BuildStatus {
        let Some(config) = resolved else {
            return BuildStatus::ConfigFileNotFound;
        };
        // A solution (no inputs of its own, only references) has nothing to build.
        if config.file_names().is_empty() && !config.parsed_config.project_references.is_empty() {
            return BuildStatus::Solution;
        }
        if self.command.build_options.force.is_true() {
            return BuildStatus::ForceBuild;
        }

        let build_info_path =
            get_build_info_file_name(config.compiler_options(), &self.compare_paths_options);
        let build_info = read_build_info(self.host.as_ref(), &build_info_path);
        let build_info_time = self
            .get_mtime(&build_info_path)
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let inputs: Vec<InputFile> = config
            .file_names()
            .iter()
            .map(|file| InputFile {
                file_name: file.clone(),
                mtime: self.get_mtime(file),
                current_text: self.host.fs().read_file(file),
            })
            .collect();

        match get_up_to_date_status(&inputs, build_info.as_ref(), build_info_time) {
            UpToDateStatusType::UpToDate | UpToDateStatusType::UpToDateWithInputFileText => {
                BuildStatus::UpToDate {
                    newest_input: newest_input(&inputs).unwrap_or_default(),
                    output: build_info_path,
                }
            }
            UpToDateStatusType::OutputMissing => BuildStatus::OutputMissing {
                output: build_info_path,
            },
            UpToDateStatusType::InputFileMissing => BuildStatus::InputFileMissing {
                input: inputs
                    .iter()
                    .find(|i| i.mtime.is_none())
                    .map(|i| i.file_name.clone())
                    .unwrap_or_default(),
            },
            UpToDateStatusType::InputFileNewer => BuildStatus::InputFileNewer {
                input: inputs
                    .iter()
                    .find(|i| i.mtime.is_some_and(|t| t > build_info_time))
                    .map(|i| i.file_name.clone())
                    .unwrap_or_default(),
                output: build_info_path,
            },
            UpToDateStatusType::TsVersionOutputOfDate => BuildStatus::TsVersionOutputOfDate {
                build_version: build_info.map(|b| b.version).unwrap_or_default(),
            },
        }
    }

    /// Handles statuses that need no build, returning the project's exit status;
    /// returns `None` when a real build is required.
    // Go: internal/execute/build/buildtask.go:BuildTask.handleStatusThatDoesntRequireBuild
    fn handle_status_that_doesnt_require_build(
        &self,
        config_name: &str,
        status: &BuildStatus,
        all_diagnostics: &mut Vec<ReportedDiagnostic>,
    ) -> Option<ExitStatus> {
        match status {
            BuildStatus::ConfigFileNotFound => {
                let diagnostic = self.compiler_diagnostic(
                    &tsgo_diagnostics::FILE_0_NOT_FOUND,
                    vec![config_name.to_string()],
                );
                self.report_diagnostic.report(self.sys, &diagnostic);
                all_diagnostics.push(diagnostic);
                Some(ExitStatus::DiagnosticsPresentOutputsSkipped)
            }
            BuildStatus::Solution => Some(ExitStatus::Success),
            BuildStatus::UpToDate { .. } => {
                if self.command.build_options.dry.is_true() {
                    self.report_builder_status(
                        &tsgo_diagnostics::PROJECT_0_IS_UP_TO_DATE,
                        vec![config_name.to_string()],
                    );
                }
                Some(ExitStatus::Success)
            }
            // Every other status needs a build.
            _ => {
                if self.command.build_options.dry.is_true() {
                    self.report_builder_status(
                        &tsgo_diagnostics::A_NON_DRY_BUILD_WOULD_BUILD_PROJECT_0,
                        vec![config_name.to_string()],
                    );
                    return Some(ExitStatus::Success);
                }
                None
            }
        }
    }

    /// Builds the project's program, emits its outputs and `.tsbuildinfo`,
    /// reports its diagnostics, and returns its exit status.
    // Go: internal/execute/build/buildtask.go:BuildTask.compileAndEmit
    fn compile_and_emit(
        &self,
        config_name: &str,
        config: &ParsedCommandLine,
        all_diagnostics: &mut Vec<ReportedDiagnostic>,
    ) -> ExitStatus {
        if self.command.build_options.verbose.is_true() {
            self.report_builder_status(
                &tsgo_diagnostics::BUILDING_PROJECT_0,
                vec![self.relative_file_name(config_name)],
            );
        }

        let mut program = new_program(ProgramOptions {
            host: self.host.clone(),
            config: Arc::new(config.clone()),
            single_threaded: true,
        });
        // Go uses `tsc.QuietDiagnosticsReporter` per project; the single
        // build-wide summary is reported once at the end.
        let quiet_summary = ReportErrorSummary::quiet();
        let result = emit_and_report_statistics(
            self.sys,
            &mut program,
            config,
            &self.report_diagnostic,
            &quiet_summary,
            &self.locale,
        );
        all_diagnostics.extend(result.diagnostics.iter().cloned());

        // Write the `.tsbuildinfo` (the incremental program's emit step; the JS /
        // `.d.ts` were already written by `program.emit` above).
        IncrementalProgram::new(&program).emit_build_info();

        result.status
    }

    /// Reports the verbose up-to-date status line for a project (a no-op unless
    /// `--verbose`).
    // Go: internal/execute/build/buildtask.go:BuildTask.reportUpToDateStatus
    fn report_up_to_date_status(&self, config_name: &str, status: &BuildStatus) {
        if !self.command.build_options.verbose.is_true() {
            return;
        }
        let relative = self.relative_file_name(config_name);
        match status {
            BuildStatus::ConfigFileNotFound => self.report_builder_status(
                &tsgo_diagnostics::PROJECT_0_IS_OUT_OF_DATE_BECAUSE_CONFIG_FILE_DOES_NOT_EXIST,
                vec![relative],
            ),
            BuildStatus::UpToDate {
                newest_input,
                output,
            } => self.report_builder_status(
                &tsgo_diagnostics::PROJECT_0_IS_UP_TO_DATE_BECAUSE_NEWEST_INPUT_1_IS_OLDER_THAN_OUTPUT_2,
                vec![
                    relative,
                    self.relative_file_name(newest_input),
                    self.relative_file_name(output),
                ],
            ),
            BuildStatus::OutputMissing { output } => self.report_builder_status(
                &tsgo_diagnostics::PROJECT_0_IS_OUT_OF_DATE_BECAUSE_OUTPUT_FILE_1_DOES_NOT_EXIST,
                vec![relative, self.relative_file_name(output)],
            ),
            BuildStatus::InputFileMissing { input } => self.report_builder_status(
                &tsgo_diagnostics::PROJECT_0_IS_OUT_OF_DATE_BECAUSE_INPUT_1_DOES_NOT_EXIST,
                vec![relative, self.relative_file_name(input)],
            ),
            BuildStatus::InputFileNewer { input, output } => self.report_builder_status(
                &tsgo_diagnostics::PROJECT_0_IS_OUT_OF_DATE_BECAUSE_OUTPUT_1_IS_OLDER_THAN_INPUT_2,
                vec![
                    relative,
                    self.relative_file_name(output),
                    self.relative_file_name(input),
                ],
            ),
            BuildStatus::TsVersionOutputOfDate { build_version } => self.report_builder_status(
                &tsgo_diagnostics::PROJECT_0_IS_OUT_OF_DATE_BECAUSE_OUTPUT_FOR_IT_WAS_GENERATED_WITH_VERSION_1_THAT_DIFFERS_WITH_CURRENT_VERSION_2,
                vec![
                    relative,
                    self.relative_file_name(build_version),
                    tsgo_core::version::version().to_string(),
                ],
            ),
            BuildStatus::ForceBuild => self.report_builder_status(
                &tsgo_diagnostics::PROJECT_0_IS_BEING_FORCIBLY_REBUILT,
                vec![relative],
            ),
            // A solution does not report a status.
            BuildStatus::Solution => {}
        }
    }

    /// Reports a time-stamped `--build` status line (`HH:MM:SS PM - <message>`),
    /// mirroring Go's `CreateBuilderStatusReporter`.
    // Go: internal/execute/tsc/diagnostics.go:CreateBuilderStatusReporter
    fn report_builder_status(&self, message: &'static Message, args: Vec<String>) {
        if self.command.compiler_options.quiet.is_true() {
            return;
        }
        let diagnostic = self.compiler_diagnostic(message, args);
        let time = format_status_time(self.sys.now());
        let format_opts = self.format_opts();
        let mut out = String::new();
        if self.should_be_pretty() {
            format_diagnostics_status_with_color_and_time(
                &mut out,
                &time,
                &diagnostic,
                &format_opts,
            );
        } else {
            format_diagnostics_status_and_time(&mut out, &time, &diagnostic, &format_opts);
        }
        out.push_str(&format_opts.new_line);
        out.push_str(&format_opts.new_line);
        self.sys.write(&out);
    }

    /// Builds a global (file-less) compiler diagnostic from a message + args,
    /// localizing it with the run locale.
    // Go: internal/ast/diagnostic.go:NewCompilerDiagnostic
    fn compiler_diagnostic(
        &self,
        message: &'static Message,
        args: Vec<String>,
    ) -> ReportedDiagnostic {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        ReportedDiagnostic {
            code: message.code(),
            category: message.category(),
            message: message.localize(&self.locale, &arg_refs),
            pos: 0,
            len: 0,
            file: None,
            message_chain: Vec::new(),
            related_information: Vec::new(),
        }
    }

    /// Wraps a project-reference diagnostic (e.g. `TS6202`) as a reportable
    /// diagnostic.
    // Go: internal/diagnosticwriter/diagnosticwriter.go:WrapASTDiagnostic
    fn project_ref_diagnostic(
        &self,
        diagnostic: &ProjectReferenceDiagnostic,
    ) -> ReportedDiagnostic {
        ReportedDiagnostic {
            code: diagnostic.code(),
            category: diagnostic.message.category(),
            message: diagnostic.text(),
            pos: 0,
            len: 0,
            file: None,
            message_chain: Vec::new(),
            related_information: Vec::new(),
        }
    }

    fn format_opts(&self) -> FormattingOptions {
        FormattingOptions {
            locale: self.locale.clone(),
            compare_paths_options: self.compare_paths_options.clone(),
            new_line: "\n".to_string(),
        }
    }

    /// Reports whether `--build` status lines should be styled with colour.
    // Go: internal/execute/tsc/diagnostics.go:shouldBePretty
    fn should_be_pretty(&self) -> bool {
        if self.command.compiler_options.pretty.is_unknown() {
            if !self.sys.get_environment_variable("NO_COLOR").is_empty() {
                return false;
            }
            if !self.sys.get_environment_variable("FORCE_COLOR").is_empty() {
                return true;
            }
            return self.sys.write_output_is_tty();
        }
        self.command.compiler_options.pretty.is_true()
    }

    // Go: internal/execute/build/orchestrator.go:Orchestrator.relativeFileName
    fn relative_file_name(&self, file_name: &str) -> String {
        convert_to_relative_path(file_name, &self.compare_paths_options)
    }

    // Go: internal/execute/build/orchestrator.go:Orchestrator.toPath
    fn to_path(&self, file_name: &str) -> Path {
        tsgo_tspath::to_path(
            file_name,
            &self.compare_paths_options.current_directory,
            self.compare_paths_options.use_case_sensitive_file_names,
        )
    }

    /// The on-disk modification time of `file`, or `None` if it does not exist.
    // Go: internal/execute/build/host.go:host.GetMTime
    fn get_mtime(&self, file: &str) -> Option<SystemTime> {
        self.host.fs().stat(file).map(|info| info.mod_time())
    }
}

/// The file name of the newest input (Go's `newestInputFileAndTime`), keeping
/// the first of any equal-mtime ties (matching Go's `inputTime.After(...)`).
// Go: internal/execute/build/buildtask.go:getUpToDateStatus (newestInputFileAndTime)
fn newest_input(inputs: &[InputFile]) -> Option<String> {
    let mut newest: Option<(SystemTime, &str)> = None;
    for input in inputs {
        if let Some(time) = input.mtime {
            match newest {
                Some((newest_time, _)) if time > newest_time => {
                    newest = Some((time, &input.file_name));
                }
                None => newest = Some((time, &input.file_name)),
                _ => {}
            }
        }
    }
    newest.map(|(_, name)| name.to_string())
}

/// Formats `time` as Go's `03:04:05 PM` (zero-padded 12-hour clock), computed
/// from the UTC time-of-day so it needs no calendar/timezone dependency.
///
/// Side effects: none (pure).
// Go: internal/execute/tsc/diagnostics.go:CreateBuilderStatusReporter (sys.Now().Format)
fn format_status_time(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let secs_of_day = (secs % 86_400) as u32;
    let hour24 = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    let period = if hour24 < 12 { "AM" } else { "PM" };
    let hour12 = match hour24 % 12 {
        0 => 12,
        h => h,
    };
    format!("{hour12:02}:{minute:02}:{second:02} {period}")
}

#[cfg(test)]
#[path = "orchestrator_test.rs"]
mod tests;
