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
        "list",
        "show",
        "add",
        "move",
        "median",
        "cargo run",
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
