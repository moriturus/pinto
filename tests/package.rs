use std::path::Path;
use std::process::Command;

fn packaged_files() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("cargo")
        .args(["package", "--list", "--allow-dirty", "--locked"])
        .current_dir(root)
        .output()
        .expect("run cargo package --list");
    assert!(
        output.status.success(),
        "cargo package --list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("cargo package file list is UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn published_package_keeps_the_pinto_library_and_binary_names() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata is valid JSON");
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| packages.first())
        .expect("metadata contains the root package");
    assert_eq!(package["name"], "pinto-cli");

    let targets = package["targets"]
        .as_array()
        .expect("root package has targets");
    assert!(targets.iter().any(|target| {
        target["name"] == "pinto"
            && target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "lib"))
    }));
    assert!(targets.iter().any(|target| {
        target["name"] == "pinto"
            && target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
    }));
}

#[test]
fn package_contains_only_runtime_files() {
    let files = packaged_files();
    let forbidden_prefixes = [
        ".pinto/", ".serena/", ".github/", "demos/", "docs/", "fuzz/", "skills/", "tests/",
    ];
    let forbidden_files = [
        "AGENTS.base.md",
        "CHANGELOG.md",
        "CLAUDE.md",
        "CODE_OF_CONDUCT.md",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "book.toml",
        "deny.toml",
        "mise.toml",
    ];

    for file in &files {
        assert!(
            !forbidden_prefixes
                .iter()
                .any(|prefix| file.starts_with(prefix)),
            "repository-only path was packaged: {file}"
        );
        assert!(
            !forbidden_files.contains(&file.as_str()),
            "repository-only file was packaged: {file}"
        );
    }

    for required in [
        "Cargo.lock",
        "Cargo.toml",
        "LICENSE",
        "README.md",
        "examples/rank_bench.rs",
        "locales/en-US.ftl",
        "locales/ja-JP.ftl",
        "src/lib.rs",
        "src/main.rs",
    ] {
        assert!(
            files.iter().any(|file| file == required),
            "missing {required}"
        );
    }

    assert!(
        files.len() <= 100,
        "package file count unexpectedly grew to {}",
        files.len()
    );
}
