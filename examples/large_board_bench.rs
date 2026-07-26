//! Measure file-backend command latency on generated large boards.
//!
//! The benchmark creates temporary boards and invokes the built `pinto` binary directly for each
//! measured command. Setup is outside the timed section, so every sample starts with the same
//! imported board size. No repository `.pinto` files are read or edited by this example.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use pinto::rank::Rank;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;
use tempfile::tempdir;

const DEFAULT_SIZES: &[usize] = &[1_000, 10_000];
const DEFAULT_SAMPLES: usize = 3;
const DEFAULT_TOLERANCE_PERCENT: f64 = 20.0;
const FIXED_TIMESTAMP: &str = "2026-01-01T00:00:00+00:00";
const BENCHMARK_COMMANDS: &[&str] = &["list", "show", "add", "move", "doctor", "import"];

#[derive(Debug)]
struct Options {
    sizes: Vec<usize>,
    samples: usize,
    baseline: Option<PathBuf>,
    output: Option<PathBuf>,
    tolerance_percent: f64,
    binary: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Environment {
    os: String,
    arch: String,
    rustc: String,
    profile: String,
    ci: String,
    runner: String,
    commit: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BenchmarkReport {
    schema: u32,
    generated_at: String,
    environment: Environment,
    samples: usize,
    tolerance_percent: f64,
    measurements: BTreeMap<usize, BoardMeasurements>,
    #[serde(default)]
    regressions: Vec<Regression>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BoardMeasurements {
    items: usize,
    commands: BTreeMap<String, CommandMeasurement>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CommandMeasurement {
    samples_ms: Vec<f64>,
    median_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Regression {
    items: usize,
    command: String,
    baseline_ms: f64,
    measured_ms: f64,
    delta_percent: f64,
}

fn main() -> Result<()> {
    let Some(options) = Options::parse(env::args().skip(1))? else {
        return Ok(());
    };

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let binary = options
        .binary
        .clone()
        .or_else(|| env::var_os("PINTO_BIN").map(PathBuf::from))
        .unwrap_or_else(|| default_binary(&root));
    if !binary.is_file() {
        bail!(
            "pinto benchmark binary was not found at {}; build it with `cargo build --release --locked --bin pinto` or set PINTO_BIN",
            binary.display()
        );
    }

    let workspace = tempdir().context("create benchmark workspace")?;
    let snapshot_path = workspace.path().join("snapshot.json");
    let mut measurements = BTreeMap::new();

    for &size in &options.sizes {
        let snapshot = generated_snapshot(size);
        fs::write(&snapshot_path, serde_json::to_vec(&snapshot)?)
            .context("write generated export snapshot")?;
        measurements.insert(
            size,
            measure_size(
                &binary,
                workspace.path(),
                &snapshot_path,
                size,
                options.samples,
            )?,
        );
    }

    let environment = collect_environment();
    let baseline = options.baseline.as_deref().map(read_report).transpose()?;
    let regressions = baseline
        .as_ref()
        .map(|baseline| {
            validate_baseline_environment(&environment, &baseline.environment)?;
            compare_reports(&measurements, baseline, options.tolerance_percent)
        })
        .transpose()?
        .unwrap_or_default();
    let report = BenchmarkReport {
        schema: 1,
        generated_at: Utc::now().to_rfc3339(),
        environment,
        samples: options.samples,
        tolerance_percent: options.tolerance_percent,
        measurements,
        regressions,
    };

    if let Some(output) = options.output.as_deref() {
        write_report(output, &report)?;
    }
    print_report(&report, baseline.is_some());
    if !report.regressions.is_empty() {
        bail!(
            "{}",
            format_regressions(
                &report.regressions,
                report.tolerance_percent,
                &report.environment
            )
        );
    }

    Ok(())
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Option<Self>> {
        let mut sizes = None;
        let mut samples = DEFAULT_SAMPLES;
        let mut baseline = None;
        let mut output = None;
        let mut tolerance_percent = DEFAULT_TOLERANCE_PERCENT;
        let mut binary = None;
        let mut arguments = arguments.into_iter();

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--help" | "-h" => {
                    println!(
                        "Usage: large_board_bench [--sizes N,...] [--samples N] [--baseline PATH] [--output PATH] [--tolerance PERCENT] [--binary PATH]\n\nDefaults: --sizes 1000,10000 --samples 3 --tolerance 20"
                    );
                    return Ok(None);
                }
                "--sizes" => {
                    let value = next_value(&mut arguments, "--sizes")?;
                    let parsed = value
                        .split(',')
                        .map(|size| {
                            size.parse::<usize>()
                                .with_context(|| format!("invalid board size {size:?}"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if parsed.is_empty() || parsed.contains(&0) {
                        bail!("--sizes must contain only positive integers");
                    }
                    sizes = Some(parsed);
                }
                "--samples" => {
                    let value = next_value(&mut arguments, "--samples")?;
                    samples = value
                        .parse::<usize>()
                        .with_context(|| format!("invalid sample count {value:?}"))?;
                    if samples == 0 {
                        bail!("--samples must be positive");
                    }
                }
                "--baseline" => {
                    baseline = Some(PathBuf::from(next_value(&mut arguments, "--baseline")?));
                }
                "--output" => {
                    output = Some(PathBuf::from(next_value(&mut arguments, "--output")?));
                }
                "--tolerance" => {
                    let value = next_value(&mut arguments, "--tolerance")?;
                    tolerance_percent = value
                        .parse::<f64>()
                        .with_context(|| format!("invalid tolerance {value:?}"))?;
                    if !tolerance_percent.is_finite() || tolerance_percent < 0.0 {
                        bail!("--tolerance must be a finite non-negative percentage");
                    }
                }
                "--binary" => {
                    binary = Some(PathBuf::from(next_value(&mut arguments, "--binary")?));
                }
                unknown => bail!("unknown option {unknown:?}; use --help for usage"),
            }
        }

        Ok(Some(Self {
            sizes: sizes.unwrap_or_else(|| DEFAULT_SIZES.to_vec()),
            samples,
            baseline,
            output,
            tolerance_percent,
            binary,
        }))
    }
}

fn next_value(arguments: &mut impl Iterator<Item = String>, option: &str) -> Result<String> {
    arguments
        .next()
        .with_context(|| format!("{option} requires a value"))
}

fn default_binary(root: &Path) -> PathBuf {
    let name = if cfg!(windows) { "pinto.exe" } else { "pinto" };
    root.join("target").join("release").join(name)
}

fn measure_size(
    binary: &Path,
    workspace: &Path,
    snapshot_path: &Path,
    size: usize,
    samples: usize,
) -> Result<BoardMeasurements> {
    let mut commands = BTreeMap::new();
    for &command in BENCHMARK_COMMANDS {
        let mut samples_ms = Vec::with_capacity(samples);
        for sample in 0..samples {
            let board = workspace.join(format!("board-{size}-{command}-{sample}"));
            fs::create_dir(&board)
                .with_context(|| format!("create benchmark board {}", board.display()))?;
            run_pinto(binary, &board, &command_args(&["init"]))?;
            if command != "import" {
                run_pinto(
                    binary,
                    &board,
                    &[
                        OsString::from("import"),
                        snapshot_path.as_os_str().to_os_string(),
                    ],
                )?;
            }
            let arguments = benchmark_args(command, size, snapshot_path);
            samples_ms.push(measure(binary, &board, &arguments)?);
        }
        let median_ms = median(&samples_ms)?;
        commands.insert(
            command.to_string(),
            CommandMeasurement {
                samples_ms,
                median_ms,
            },
        );
    }
    Ok(BoardMeasurements {
        items: size,
        commands,
    })
}

fn benchmark_args(command: &str, size: usize, snapshot_path: &Path) -> Vec<OsString> {
    match command {
        "list" => command_args(&["list", "--json"]),
        "show" => command_args(&["show", &format!("T-{}", size / 2 + 1), "--json"]),
        "add" => command_args(&["add", "Measured benchmark item"]),
        "move" => command_args(&["move", "T-1", "in-progress"]),
        "doctor" => command_args(&["doctor"]),
        "import" => vec![
            OsString::from("import"),
            snapshot_path.as_os_str().to_os_string(),
        ],
        _ => unreachable!("benchmark command is declared in BENCHMARK_COMMANDS"),
    }
}

fn command_args(arguments: &[&str]) -> Vec<OsString> {
    arguments.iter().map(OsString::from).collect()
}

fn measure(binary: &Path, board: &Path, arguments: &[OsString]) -> Result<f64> {
    let started = Instant::now();
    run_pinto(binary, board, arguments)?;
    Ok(started.elapsed().as_secs_f64() * 1_000.0)
}

fn run_pinto(binary: &Path, board: &Path, arguments: &[OsString]) -> Result<Output> {
    let output = Command::new(binary)
        .arg("--dir")
        .arg(board)
        .args(arguments)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .output()
        .with_context(|| format!("run pinto for board {}", board.display()))?;
    if !output.status.success() {
        bail!(
            "pinto command failed in {} with {:?}\nstdout:\n{}\nstderr:\n{}",
            board.display(),
            arguments,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

fn generated_snapshot(size: usize) -> Value {
    let mut previous = None;
    let items = (1..=size)
        .map(|number| {
            let rank = Rank::after(previous.as_ref());
            previous = Some(rank.clone());
            json!({
                "id": format!("T-{number}"),
                "title": format!("Generated benchmark item {number}"),
                "status": "todo",
                "rank": rank.as_str(),
                "points": null,
                "labels": [],
                "assignee": null,
                "sprint": null,
                "parent": null,
                "depends_on": [],
                "start_at": null,
                "done_at": null,
                "commits": [],
                "created": FIXED_TIMESTAMP,
                "updated": FIXED_TIMESTAMP,
                "body": ""
            })
        })
        .collect::<Vec<_>>();

    json!({
        "items": items,
        "sprints": [],
        "config": {
            "columns": ["todo", "in-progress", "review", "done"],
            "display": {"markdown": true, "timezone": "local"},
            "done_column": "done",
            "points": {"aggregate_children": false},
            "project": {"key": "T", "name": "large-board-benchmark"},
            "storage": {"backend": "file"},
            "tui": {"confirm_quit": true},
            "wip": {"enabled": true}
        },
        "dod": null
    })
}

fn median(values: &[f64]) -> Result<f64> {
    if values.is_empty() {
        bail!("cannot calculate a median without samples");
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    Ok(sorted[sorted.len() / 2])
}

fn collect_environment() -> Environment {
    Environment {
        os: env::consts::OS.to_string(),
        arch: env::consts::ARCH.to_string(),
        rustc: command_text("rustc", &["-vV"]).unwrap_or_else(|| "unknown".to_string()),
        profile: "release".to_string(),
        ci: environment_value("CI", "local"),
        runner: environment_value("GITHUB_RUNNER_NAME", "local"),
        commit: environment_value(
            "GITHUB_SHA",
            command_text("git", &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string()),
        ),
    }
}

fn environment_value(name: &str, fallback: impl Into<String>) -> String {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.into())
}

fn command_text(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .trim()
        .replace('\n', "; ");
    (!text.is_empty()).then_some(text)
}

fn read_report(path: &Path) -> Result<BenchmarkReport> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("read benchmark baseline {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parse benchmark baseline {}", path.display()))
}

fn validate_baseline_environment(measured: &Environment, baseline: &Environment) -> Result<()> {
    if measured.os != baseline.os
        || measured.arch != baseline.arch
        || measured.profile != baseline.profile
    {
        bail!(
            "baseline environment is incompatible: baseline={}/{} profile={} measured={}/{} profile={}; use a baseline collected on the same OS, architecture, and profile",
            baseline.os,
            baseline.arch,
            baseline.profile,
            measured.os,
            measured.arch,
            measured.profile
        );
    }
    Ok(())
}

fn write_report(path: &Path, report: &BenchmarkReport) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create benchmark report directory {}", parent.display()))?;
    }
    let contents = serde_json::to_vec_pretty(report)?;
    fs::write(path, contents).with_context(|| format!("write benchmark report {}", path.display()))
}

fn compare_reports(
    measurements: &BTreeMap<usize, BoardMeasurements>,
    baseline: &BenchmarkReport,
    tolerance_percent: f64,
) -> Result<Vec<Regression>> {
    if baseline.schema != 1 {
        bail!(
            "unsupported benchmark baseline schema {}; expected 1",
            baseline.schema
        );
    }
    if !tolerance_percent.is_finite() || tolerance_percent < 0.0 {
        bail!("benchmark tolerance must be a finite non-negative percentage");
    }

    let mut regressions = Vec::new();
    for (items, measured_board) in measurements {
        let baseline_board = baseline
            .measurements
            .get(items)
            .with_context(|| format!("benchmark baseline has no {items}-item measurements"))?;
        for (command, measured) in &measured_board.commands {
            let baseline_measurement = baseline_board.commands.get(command).with_context(|| {
                format!("benchmark baseline has no {command} measurement for {items} items")
            })?;
            if !baseline_measurement.median_ms.is_finite() || baseline_measurement.median_ms <= 0.0
            {
                bail!(
                    "benchmark baseline for {command} at {items} items must have a positive finite median"
                );
            }
            let delta_percent = (measured.median_ms - baseline_measurement.median_ms)
                / baseline_measurement.median_ms
                * 100.0;
            if delta_percent > tolerance_percent {
                regressions.push(Regression {
                    items: *items,
                    command: command.clone(),
                    baseline_ms: baseline_measurement.median_ms,
                    measured_ms: measured.median_ms,
                    delta_percent,
                });
            }
        }
    }
    Ok(regressions)
}

fn print_report(report: &BenchmarkReport, compared_with_baseline: bool) {
    println!("# Large-board file-backend benchmark");
    println!(
        "environment: {}/{} runner={} rustc={} commit={}",
        report.environment.os,
        report.environment.arch,
        report.environment.runner,
        report.environment.rustc,
        report.environment.commit
    );
    println!(
        "statistic: median of {} samples; setup is excluded from command timing",
        report.samples
    );
    if compared_with_baseline {
        println!(
            "regression gate: baseline comparison with {:.1}% tolerance",
            report.tolerance_percent
        );
    }
    println!();
    println!(
        "| items | list | show | add | move | doctor | import |\n| ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
    );
    for board in report.measurements.values() {
        let value = |command: &str| {
            board
                .commands
                .get(command)
                .map(|measurement| format_millis(measurement.median_ms))
                .unwrap_or_else(|| "-".to_string())
        };
        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            format_count(board.items),
            value("list"),
            value("show"),
            value("add"),
            value("move"),
            value("doctor"),
            value("import")
        );
    }
    if let Some(first) = report.measurements.values().next() {
        println!();
        println!(
            "samples and medians are stored for {} command measurements; generated at {}",
            first.commands.len(),
            report.generated_at
        );
    }
}

fn format_regressions(
    regressions: &[Regression],
    tolerance_percent: f64,
    environment: &Environment,
) -> String {
    let mut message = format!(
        "performance regressions detected (tolerance={tolerance_percent:.1}%; environment={}/{} runner={} rustc={})",
        environment.os, environment.arch, environment.runner, environment.rustc
    );
    for regression in regressions {
        message.push_str(&format!(
            "\n- {} items / {}: baseline={:.1} ms, measured={:.1} ms, delta={:.1}%",
            format_count(regression.items),
            regression.command,
            regression.baseline_ms,
            regression.measured_ms,
            regression.delta_percent
        ));
    }
    message
}

fn format_millis(milliseconds: f64) -> String {
    format!("{milliseconds:.1} ms")
}

fn format_count(value: usize) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn environment() -> Environment {
        Environment {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            rustc: "rustc test".to_string(),
            profile: "release".to_string(),
            ci: "github-actions".to_string(),
            runner: "ubuntu-latest".to_string(),
            commit: "abc123".to_string(),
        }
    }

    fn report(median_ms: f64) -> BenchmarkReport {
        BenchmarkReport {
            schema: 1,
            generated_at: "2026-07-26T00:00:00Z".to_string(),
            environment: environment(),
            samples: 3,
            tolerance_percent: 10.0,
            measurements: BTreeMap::from([(
                1_000,
                BoardMeasurements {
                    items: 1_000,
                    commands: BTreeMap::from([(
                        "doctor".to_string(),
                        CommandMeasurement {
                            samples_ms: vec![median_ms - 1.0, median_ms, median_ms + 1.0],
                            median_ms,
                        },
                    )]),
                },
            )]),
            regressions: Vec::new(),
        }
    }

    #[test]
    fn options_parse_benchmark_and_regression_settings() {
        let options = Options::parse([
            "--sizes".to_string(),
            "1000,10000".to_string(),
            "--samples".to_string(),
            "5".to_string(),
            "--baseline".to_string(),
            "baseline.json".to_string(),
            "--output".to_string(),
            "report.json".to_string(),
            "--tolerance".to_string(),
            "12.5".to_string(),
        ])
        .expect("options parse")
        .expect("benchmark should run");

        assert_eq!(options.sizes, vec![1_000, 10_000]);
        assert_eq!(options.samples, 5);
        assert_eq!(
            options.baseline.as_deref(),
            Some(Path::new("baseline.json"))
        );
        assert_eq!(options.output.as_deref(), Some(Path::new("report.json")));
        assert_eq!(options.tolerance_percent, 12.5);
    }

    #[test]
    fn regression_diagnostic_identifies_command_values_and_environment() {
        let baseline = report(100.0);
        let measured = report(130.0);
        let regressions =
            compare_reports(&measured.measurements, &baseline, 10.0).expect("compare reports");

        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].command, "doctor");
        assert_eq!(regressions[0].baseline_ms, 100.0);
        assert_eq!(regressions[0].measured_ms, 130.0);

        let diagnostic = format_regressions(&regressions, 10.0, &measured.environment);
        for marker in [
            "doctor",
            "1,000",
            "baseline=100.0 ms",
            "measured=130.0 ms",
            "environment=linux/x86_64",
        ] {
            assert!(diagnostic.contains(marker), "diagnostic omits {marker}");
        }
    }

    #[test]
    fn incompatible_baseline_environment_is_rejected_before_comparison() {
        let mut baseline = report(100.0);
        baseline.environment.os = "macos".to_string();
        baseline.environment.arch = "aarch64".to_string();
        let measured = report(100.0);

        let error = validate_baseline_environment(&measured.environment, &baseline.environment)
            .expect_err("incompatible environments must be rejected");

        let message = error.to_string();
        assert!(message.contains("baseline environment is incompatible"));
        assert!(message.contains("baseline=macos/aarch64"));
        assert!(message.contains("measured=linux/x86_64"));
    }

    #[test]
    fn report_serializes_medians_samples_and_environment() {
        let json = serde_json::to_string(&report(100.0)).expect("serialize report");
        for marker in [
            "\"median_ms\"",
            "\"samples_ms\"",
            "\"environment\"",
            "\"doctor\"",
        ] {
            assert!(json.contains(marker), "report omits {marker}");
        }
    }
}
