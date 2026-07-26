//! Integration checks for the large-board benchmark contract.

use std::fs;
use std::path::Path;

fn repository_file(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

#[test]
fn large_board_benchmark_documents_a_reproducible_file_backend_run() {
    let benchmarks = repository_file("docs/benchmarks.md");
    for marker in [
        "# Large-board file-backend benchmarks",
        "large_board_bench",
        "1,000",
        "10,000",
        "fresh temporary board",
        "list",
        "show",
        "add",
        "move",
        "doctor",
        "import",
        "median",
        "baseline",
        "tolerance",
        "samples_ms",
        "median_ms",
        "14-day",
        "environment",
    ] {
        assert!(
            benchmarks.contains(marker),
            "benchmark guide omits {marker}"
        );
    }

    let stability = repository_file("docs/stability.md");
    for marker in [
        "## Single-item read scaling",
        "complete task and archive validation",
        "fail-fast",
    ] {
        assert!(stability.contains(marker), "stability guide omits {marker}");
    }

    let demo = repository_file("demos/single/large-board-benchmark/README.md");
    for marker in [
        "large_board_bench",
        "cargo run --manifest-path ../../../Cargo.toml",
        "list",
        "show",
        "add",
        "move",
    ] {
        assert!(demo.contains(marker), "benchmark demo omits {marker}");
    }
}

#[test]
fn large_board_benchmark_has_ci_baseline_and_failure_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let runner = fs::read_to_string(root.join("examples/large_board_bench.rs"))
        .expect("read large-board benchmark runner");
    for marker in [
        "doctor",
        "import",
        "--baseline",
        "--tolerance",
        "median_ms",
        "samples_ms",
        "environment",
    ] {
        assert!(runner.contains(marker), "benchmark runner omits {marker}");
    }

    let workflow =
        fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read CI workflow");
    for marker in [
        "large-board-smoke",
        "large-board-scheduled",
        "--sizes 1000",
        "--sizes 10000",
        "large-board-baseline-linux-x86_64.json",
        "tolerance=20",
        "ACT:-",
        "actions/upload-artifact",
        "env.ACT",
        "retention-days",
    ] {
        assert!(workflow.contains(marker), "CI workflow omits {marker}");
    }

    let baseline = fs::read_to_string(root.join("benchmarks/large-board-baseline.json"))
        .expect("read committed benchmark baseline");
    let report: serde_json::Value =
        serde_json::from_str(&baseline).expect("benchmark baseline is valid JSON");
    for size in ["1000", "10000"] {
        let board = &report["measurements"][size];
        assert_eq!(
            board["items"],
            size.parse::<u64>().expect("size is numeric")
        );
        for command in ["list", "show", "add", "move", "doctor", "import"] {
            assert!(
                board["commands"][command]["median_ms"].is_number(),
                "baseline omits {size}-item {command} median"
            );
            assert!(
                board["commands"][command]["samples_ms"].is_array(),
                "baseline omits {size}-item {command} samples"
            );
        }
    }
    for marker in [
        "\"measurements\"",
        "\"1000\"",
        "\"10000\"",
        "\"environment\"",
    ] {
        assert!(baseline.contains(marker), "baseline omits {marker}");
    }

    let linux_baseline =
        fs::read_to_string(root.join("benchmarks/large-board-baseline-linux-x86_64.json"))
            .expect("read Linux benchmark baseline");
    let linux_report: serde_json::Value =
        serde_json::from_str(&linux_baseline).expect("Linux benchmark baseline is valid JSON");
    assert_eq!(linux_report["environment"]["os"], "linux");
    assert_eq!(linux_report["environment"]["arch"], "x86_64");
}
