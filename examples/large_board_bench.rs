//! Measure file-backend command latency on generated large boards.
//!
//! The benchmark deliberately invokes the CLI through `cargo run` so the documented command is
//! also the path used for the measurements. Generated board data lives in a temporary directory;
//! no repository `.pinto` files are read or edited by this example.

use anyhow::{Context, Result, bail};
use pinto::rank::Rank;
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};
use tempfile::tempdir;

const DEFAULT_SIZES: &[usize] = &[1_000, 10_000];
const DEFAULT_SAMPLES: usize = 3;
const FIXED_TIMESTAMP: &str = "2026-01-01T00:00:00+00:00";

#[derive(Debug)]
struct Options {
    sizes: Vec<usize>,
    samples: usize,
}

#[derive(Debug)]
struct Measurements {
    list: Duration,
    show: Duration,
    add: Duration,
    move_item: Duration,
}

fn main() -> Result<()> {
    let Some(options) = Options::parse(env::args().skip(1))? else {
        return Ok(());
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let workspace = tempdir().context("create benchmark workspace")?;
    let snapshot_path = workspace.path().join("snapshot.json");

    println!("# Large-board file-backend benchmark");
    println!(
        "runner: cargo run --quiet --release --locked --manifest-path {}",
        manifest.display()
    );
    println!(
        "statistic: median of {} samples; timing includes CLI startup and cargo-run dispatch",
        options.samples
    );
    println!();
    println!("| items | list | show | add | move |");
    println!("| ---: | ---: | ---: | ---: | ---: |");

    for &size in &options.sizes {
        let snapshot = generated_snapshot(size);
        fs::write(&snapshot_path, serde_json::to_vec(&snapshot)?)
            .context("write generated export snapshot")?;

        let mut samples = Vec::with_capacity(options.samples);
        for sample in 0..options.samples {
            let board = workspace
                .path()
                .join(format!("board-{size}-sample-{sample}"));
            fs::create_dir(&board)
                .with_context(|| format!("create benchmark board {}", board.display()))?;
            run_pinto(&manifest, &board, &["init"])?;
            run_pinto(
                &manifest,
                &board,
                &["import", snapshot_path.to_string_lossy().as_ref()],
            )?;

            // Compile and warm the cargo-run path before collecting the first sample.
            if sample == 0 {
                run_pinto(&manifest, &board, &["list", "--json"])?;
            }
            samples.push(measure_commands(&manifest, &board, size)?);
        }

        let list = median(samples.iter().map(|sample| sample.list));
        let show = median(samples.iter().map(|sample| sample.show));
        let add = median(samples.iter().map(|sample| sample.add));
        let move_item = median(samples.iter().map(|sample| sample.move_item));
        println!(
            "| {size} | {} | {} | {} | {} |",
            format_duration(list),
            format_duration(show),
            format_duration(add),
            format_duration(move_item)
        );
    }

    Ok(())
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Option<Self>> {
        let mut sizes = None;
        let mut samples = DEFAULT_SAMPLES;
        let mut arguments = arguments.into_iter();

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--help" | "-h" => {
                    println!(
                        "Usage: large_board_bench [--sizes N,...] [--samples N]\n\nDefaults: --sizes 1000,10000 --samples 3"
                    );
                    return Ok(None);
                }
                "--sizes" => {
                    let value = arguments
                        .next()
                        .context("--sizes requires a comma-separated value")?;
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
                    let value = arguments
                        .next()
                        .context("--samples requires a positive integer")?;
                    samples = value
                        .parse::<usize>()
                        .with_context(|| format!("invalid sample count {value:?}"))?;
                    if samples == 0 {
                        bail!("--samples must be positive");
                    }
                }
                unknown => bail!("unknown option {unknown:?}; use --help for usage"),
            }
        }

        Ok(Some(Self {
            sizes: sizes.unwrap_or_else(|| DEFAULT_SIZES.to_vec()),
            samples,
        }))
    }
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

fn measure_commands(manifest: &Path, board: &Path, size: usize) -> Result<Measurements> {
    let show_id = format!("T-{}", size / 2 + 1);
    let list = measure(manifest, board, &["list", "--json"])?;
    let show = measure(manifest, board, &["show", &show_id, "--json"])?;
    // Keep add last so every measured operation starts from the generated size. `move` would
    // otherwise observe the extra item created by add in the same sample.
    let move_item = measure(manifest, board, &["move", "T-1", "in-progress"])?;
    let add = measure(manifest, board, &["add", "Measured benchmark item"])?;
    Ok(Measurements {
        list,
        show,
        add,
        move_item,
    })
}

fn measure(manifest: &Path, board: &Path, arguments: &[&str]) -> Result<Duration> {
    let started = Instant::now();
    run_pinto(manifest, board, arguments)?;
    Ok(started.elapsed())
}

fn run_pinto(manifest: &Path, board: &Path, arguments: &[&str]) -> Result<Output> {
    let output = Command::new("cargo")
        .args(["run", "--quiet", "--release", "--locked", "--manifest-path"])
        .arg(manifest)
        .arg("--")
        .arg("--dir")
        .arg(board)
        .args(arguments)
        .output()
        .with_context(|| format!("run cargo for board {}", board.display()))?;
    if !output.status.success() {
        bail!(
            "pinto command failed in {}\nstdout:\n{}\nstderr:\n{}",
            board.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

fn median(values: impl Iterator<Item = Duration>) -> Duration {
    let mut values = values.collect::<Vec<_>>();
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    values[values.len() / 2]
}

fn format_duration(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1_000.0)
}
