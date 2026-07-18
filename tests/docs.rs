use std::fs;
use std::path::Path;

const BOOK_SOURCE: &str = "docs/book/src";

fn repository_file(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path))
        .map(|contents| contents.replace("\r\n", "\n"))
        .unwrap_or_else(|error| panic!("expected documentation file {path}: {error}"))
}

#[test]
fn mdbook_structure_covers_user_and_contributor_workflows() {
    let config = repository_file("book.toml");
    assert!(config.contains("src = \"docs/book/src\""));
    assert!(config.contains("build-dir = \"target/book\""));

    let summary = repository_file("docs/book/src/SUMMARY.md");
    for page in [
        "introduction.md",
        "installation.md",
        "quickstart.md",
        "cli.md",
        "data-format.md",
        "dogfooding.md",
        "contributing.md",
        "reproducibility.md",
    ] {
        assert!(summary.contains(page), "SUMMARY.md does not link {page}");
        let path = format!("{BOOK_SOURCE}/{page}");
        let contents = repository_file(&path);
        assert!(
            !contents.trim().is_empty(),
            "documentation page is empty: {path}"
        );
    }
}

#[test]
fn documentation_includes_the_required_operating_instructions() {
    let installation = repository_file("docs/book/src/installation.md");
    assert!(installation.contains("cargo install pinto-cli"));
    assert!(installation.contains("installs the `pinto` binary"));

    let quickstart = repository_file("docs/book/src/quickstart.md");
    for command in [
        "pinto init",
        "pinto add",
        "pinto list",
        "pinto show",
        "pinto move",
    ] {
        assert!(quickstart.contains(command), "quick start omits {command}");
    }

    let cli = repository_file("docs/book/src/cli.md");
    for command in [
        "pinto board",
        "pinto sprint",
        "pinto automate",
        "pinto list --roots-only",
        "pinto board --roots-only",
        "--json",
    ] {
        assert!(cli.contains(command), "CLI guide omits {command}");
    }

    let data_format = repository_file("docs/book/src/data-format.md");
    for term in [".pinto/", "config.toml", "TOML", "Markdown"] {
        assert!(data_format.contains(term), "data-format guide omits {term}");
    }

    let dogfooding = repository_file("docs/book/src/dogfooding.md");
    for command in ["cargo run --", "pinto list", "pinto show", "pinto move"] {
        assert!(
            dogfooding.contains(command),
            "dogfooding guide omits {command}"
        );
    }

    let contributing = repository_file("docs/book/src/contributing.md");
    for term in ["Red", "Green", "Refactor", "mise run check"] {
        assert!(
            contributing.contains(term),
            "contributing guide omits {term}"
        );
    }

    let testing = repository_file("docs/book/src/testing.md");
    for term in [
        "mise run coverage",
        "SHELL_EXIT_WAIT",
        "Cobertura line-rate",
    ] {
        assert!(testing.contains(term), "testing guide omits {term}");
    }
}

#[test]
fn markdown_parser_and_fuzz_guidance_are_wired() {
    let markdown = "+++\n\
id = \"T-1\"\n\
title = \"Parser example\"\n\
status = \"todo\"\n\
rank = \"i\"\n\
created = \"1970-01-01T00:00:00Z\"\n\
updated = \"1970-01-01T00:00:00Z\"\n\
+++\n\
Body marker\n";
    let item = pinto::storage::parse_item_markdown(markdown, Path::new("T-1.md"))
        .expect("valid Markdown frontmatter parses through the public API");
    assert_eq!(item.id.to_string(), "T-1");
    assert_eq!(item.body, "Body marker");

    let fuzz_manifest = repository_file("fuzz/Cargo.toml");
    assert!(fuzz_manifest.contains("markdown_frontmatter_parse"));
    let fuzz_target = repository_file("fuzz/fuzz_targets/markdown_frontmatter_parse.rs");
    assert!(fuzz_target.contains("parse_item_markdown"));

    let workflow = repository_file(".github/workflows/ci.yml");
    assert!(workflow.contains("cargo fuzz run ${{ matrix.target }}"));
    assert!(workflow.contains("markdown_frontmatter_parse"));
    let testing = repository_file("docs/book/src/testing.md");
    for command in ["cargo test --doc", "cargo fuzz run", "fuzz/artifacts"] {
        assert!(testing.contains(command), "testing guide omits {command}");
    }
}

#[test]
fn toolchain_locks_cargo_and_separates_ci_roles() {
    let mise = repository_file("mise.toml");
    assert!(mise.contains("rust = \"1.97.0\""));
    for command in [
        "cargo test --all-features --locked",
        "cargo clippy --all-targets --all-features --locked",
        "cargo doc --no-deps --all-features --locked",
        "cargo llvm-cov --all-features --workspace --locked",
        "-D clippy::must_use_candidate",
        "-D clippy::redundant_closure_for_method_calls",
    ] {
        assert!(mise.contains(command), "mise.toml omits {command}");
    }

    let workflow = repository_file(".github/workflows/ci.yml");
    for marker in [
        "Setup pinned development toolchain",
        "current-stable:",
        "dtolnay/rust-toolchain@stable",
        "release:",
        "cargo build --release --all-features --locked",
        "./scripts/verify-package.sh",
        "cargo install --path . --locked",
    ] {
        assert!(workflow.contains(marker), "CI workflow omits {marker}");
    }

    let installation = repository_file("docs/book/src/installation.md");
    assert!(installation.contains("cargo install --path . --locked"));
    let readme = repository_file("README.md");
    assert!(readme.contains("cargo install --path . --locked"));

    let reproducibility = repository_file("docs/book/src/reproducibility.md");
    for marker in [
        "Rust 1.97.0",
        "Cargo.lock",
        "cargo update",
        "mise run check",
        "cargo package --all-features --locked",
        "current-stable",
    ] {
        assert!(
            reproducibility.contains(marker),
            "reproducibility guide omits {marker}"
        );
    }
}

#[test]
fn allowlisted_package_is_verified_in_release_paths() {
    let manifest = repository_file("Cargo.toml");
    for marker in [
        "include = [",
        "\"/Cargo.toml\"",
        "\"/Cargo.lock\"",
        "\"/README.md\"",
        "\"/LICENSE\"",
        "\"/src/**\"",
        "\"/locales/**\"",
        "\"/examples/rank_bench.rs\"",
    ] {
        assert!(manifest.contains(marker), "Cargo.toml omits {marker}");
    }

    let verifier = repository_file("scripts/verify-package.sh");
    for marker in [
        "cargo package --all-features --locked",
        "package-files.txt",
        "cargo test --all-features --locked",
    ] {
        assert!(verifier.contains(marker), "package verifier omits {marker}");
    }
    assert!(!verifier.contains("package-size-budget.bytes"));

    let workflow = repository_file(".github/workflows/ci.yml");
    assert!(workflow.contains("./scripts/verify-package.sh"));

    let mise = repository_file("mise.toml");
    assert!(mise.contains("scripts/verify-package.sh"));

    let reproducibility = repository_file("docs/book/src/reproducibility.md");
    for marker in ["allowlisted package", "package file list", "packaged crate"] {
        assert!(
            reproducibility.contains(marker),
            "reproducibility guide omits {marker}"
        );
    }
    assert!(!reproducibility.contains("package-size-budget.bytes"));

    let demo = repository_file("demos/single/package-allowlist/README.md");
    for marker in ["cargo package --all-features --locked", "package-files.txt"] {
        assert!(demo.contains(marker), "package demo omits {marker}");
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!root.join("release/package-size-budget.bytes").exists());
    assert!(
        root.join("demos/single/package-allowlist/.pinto/tasks/T-1.md")
            .is_file()
    );
}

#[test]
fn documentation_demo_contains_a_reproducible_board_and_guide() {
    let readme = repository_file("demos/single/documentation/README.md");
    assert!(readme.contains("mdBook"));
    assert!(readme.contains("cargo run --manifest-path ../../../Cargo.toml -- init"));
    assert!(readme.contains("Review contributor guidance"));
    assert!(readme.contains("RUSTDOCFLAGS=\"-D warnings\" cargo doc --no-deps"));

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let task_dir = root.join("demos/single/documentation/.pinto/tasks");
    assert!(task_dir.join("T-1.md").is_file());
    assert!(task_dir.join("T-2.md").is_file());
    assert!(task_dir.join("T-3.md").is_file());
    assert!(task_dir.join("T-4.md").is_file());
}

#[test]
fn archive_recovery_demo_documents_listing_and_restore() {
    let readme = repository_file("demos/single/archive-recovery/README.md");
    for command in [
        "list --archived",
        "show T-1 --archived",
        "restore T-1",
        "does not overwrite either record",
    ] {
        assert!(
            readme.contains(command),
            "archive recovery demo omits {command}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let board = root.join("demos/single/archive-recovery/.pinto");
    assert!(board.join("tasks/T-2.md").is_file());
    assert!(board.join("archive/T-1.md").is_file());
}

#[test]
fn stale_filter_demo_documents_duration_queries() {
    let readme = repository_file("demos/single/stale-filter/README.md");
    for command in [
        "list --stale 1s",
        "list --stale 1s --status todo --json",
        "list --stale 1s --label backend",
    ] {
        assert!(
            readme.contains(command),
            "stale-filter demo omits {command}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let task_dir = root.join("demos/single/stale-filter/.pinto/tasks");
    for id in ["T-1", "T-2", "T-3"] {
        assert!(task_dir.join(format!("{id}.md")).is_file());
    }
}

#[test]
fn parent_child_demo_documents_root_only_views() {
    let readme = repository_file("demos/single/parent-child/README.md");
    for command in [
        "list --roots-only",
        "list --roots-only --json",
        "board --roots-only --long",
    ] {
        assert!(
            readme.contains(command),
            "parent-child demo omits {command}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let task_dir = root.join("demos/single/parent-child/.pinto/tasks");
    assert!(task_dir.join("T-1.md").is_file());
    assert!(task_dir.join("T-2.md").is_file());
    assert!(task_dir.join("T-5.md").is_file());
    assert!(task_dir.join("T-6.md").is_file());
}

#[test]
fn cookbook_covers_setup_pbi_and_sprint_recipes() {
    let summary = repository_file("docs/book/src/SUMMARY.md");
    assert!(
        summary.contains("cookbook.md"),
        "SUMMARY.md does not link cookbook.md"
    );

    let cookbook = repository_file("docs/book/src/cookbook.md");
    for command in [
        "pinto init",
        "pinto add",
        "pinto list",
        "pinto move",
        "pinto sprint new",
        "pinto sprint start",
        "pinto sprint add S-1 --status todo --limit",
        "pinto sprint velocity",
        "pinto board",
    ] {
        assert!(cookbook.contains(command), "cookbook omits {command}");
    }
}

#[test]
fn cookbook_unix_pipeline_recipes_cover_standard_tools() {
    let cookbook = repository_file("docs/book/src/cookbook.md");
    for tool in [
        "cut", "tr", "grep", "sort", "uniq", "wc", "head", "tail", "paste", "join", "sed",
    ] {
        let piped = format!("| {tool} ");
        assert!(
            cookbook.contains(&piped),
            "cookbook has no pipeline using {tool}"
        );
    }

    let unix_section = cookbook
        .split_once("## Unix text-stream recipes")
        .map(|(_, rest)| rest)
        .expect("cookbook contains a Unix text-stream recipe section");
    let recipes = unix_section.matches("\n### ").count();
    assert!(
        recipes >= 10,
        "expected at least 10 Unix text-stream recipes, found {recipes}"
    );

    for marker in ["Prerequisites", "Verify", "GNU", "BSD"] {
        assert!(
            unix_section.contains(marker),
            "Unix recipe section omits {marker}"
        );
    }
}

#[test]
fn cookbook_demo_contains_reproducible_pipeline_data() {
    let readme = repository_file("demos/single/cookbook/README.md");
    for phrase in [
        "cargo run --manifest-path ../../../Cargo.toml -- init",
        "sprint add S-1 --status todo --limit 2",
        "| cut -d' ' -f1",
        "sort | uniq -c",
        "docs/book/src/cookbook.md",
    ] {
        assert!(readme.contains(phrase), "cookbook demo omits {phrase}");
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let board = root.join("demos/single/cookbook/.pinto");
    assert!(board.join("tasks/T-1.md").is_file());
    assert!(board.join("tasks/T-5.md").is_file());
    assert!(board.join("sprints/S-1.md").is_file());
}

#[test]
fn cookbook_tail_recipe_matches_noninteractive_list_output() {
    let cookbook = repository_file("docs/book/src/cookbook.md");
    assert!(
        cookbook.contains("pinto list --long | tail -n 2"),
        "tail recipe must match pinto's noninteractive output"
    );
    assert!(
        !cookbook.contains("pinto list --long | tail -n +2"),
        "tail recipe must not treat a data row as a header"
    );
    assert!(
        cookbook.contains("noninteractive output has no header row"),
        "cookbook must document the noninteractive --long format"
    );
}

#[test]
fn cookbook_describes_the_sprint_close_sequence_accurately() {
    let cookbook = repository_file("docs/book/src/cookbook.md");
    assert!(cookbook.contains(
        "completes one Sprint PBI, rolls the unfinished PBI into the next Sprint while\nclosing"
    ));
    assert!(
        !cookbook
            .contains("Closing out the sprint moves PBIs to `done` and changes the board state")
    );
    assert!(!cookbook.contains("moves the sprint PBIs to `done` before closing the Sprint"));
}

#[test]
fn sprint_assignment_validation_demo_contains_reproducible_data_and_commands() {
    let readme = repository_file("demos/single/sprint-assignment-validation/README.md");
    for command in [
        "sprint new S-1",
        "add \"Created in an existing sprint\" --sprint S-1",
        "edit T-2 --sprint S-1",
        "sprint unassign S-1 T-2",
        "sprint add \"S 1\" T-2",
    ] {
        assert!(
            readme.contains(command),
            "sprint assignment demo omits {command}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let demo = root.join("demos/single/sprint-assignment-validation/.pinto");
    assert!(demo.join("tasks/T-1.md").is_file());
    assert!(demo.join("tasks/T-2.md").is_file());
    assert!(demo.join("sprints/S-1.md").is_file());
}

#[test]
fn i18n_demo_contains_localized_cli_examples_and_board_data() {
    let readme = repository_file("demos/single/i18n/README.md");
    for term in [
        "LC_ALL=en_US.UTF-8",
        "LC_ALL=ja_JP.UTF-8",
        "migrate --to file",
        "list --json",
        "original wording",
    ] {
        assert!(readme.contains(term), "i18n demo omits {term}");
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let task_dir = root.join("demos/single/i18n/.pinto/tasks");
    assert!(task_dir.join("T-1.md").is_file());
    assert!(task_dir.join("T-2.md").is_file());
    assert!(root.join("demos/single/i18n/.pinto/dod.md").is_file());
}

#[test]
fn i18n_localizer_cache_demo_contains_repeated_rendering_commands_and_data() {
    let readme = repository_file("demos/single/i18n-localizer-cache/README.md");
    for phrase in [
        "LC_ALL=en_US.UTF-8",
        "LC_ALL=ja_JP.UTF-8",
        "cargo run --manifest-path ../../../Cargo.toml -- list --long",
        "cargo run --manifest-path ../../../Cargo.toml -- board",
        "cargo run --manifest-path ../../../Cargo.toml -- kanban",
        "current_reuses_one_localizer_for_the_process_lifetime",
    ] {
        assert!(
            readme.contains(phrase),
            "localizer cache demo omits {phrase}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let task_dir = root.join("demos/single/i18n-localizer-cache/.pinto/tasks");
    assert!(task_dir.join("T-1.md").is_file());
    assert!(task_dir.join("T-2.md").is_file());
}

#[test]
fn kanban_documentation_covers_startup_scope_filters() {
    let readme = repository_file("README.md");
    let kanban = repository_file("docs/book/src/kanban.md");
    for option in ["--sprint", "--label", "--all-labels", "--search", "--regex"] {
        assert!(
            readme.contains(option),
            "README omits Kanban option {option}"
        );
        assert!(
            kanban.contains(option),
            "Kanban guide omits startup option {option}"
        );
    }
    assert!(
        kanban.contains("read-only"),
        "Kanban guide omits non-mutating scope"
    );
    assert!(
        kanban.contains("reloads after an edit"),
        "Kanban guide omits filter persistence"
    );
}

#[test]
fn file_id_integrity_demo_contains_active_archive_and_sprint_data() {
    let readme = repository_file("demos/single/file-id-integrity/README.md");
    for phrase in [
        "filename/frontmatter identity checks",
        "cargo run --manifest-path ../../../Cargo.toml -- list --long",
        "cargo run --manifest-path ../../../Cargo.toml -- sprint list",
        "`next_id`",
        "duplicate IDs",
    ] {
        assert!(
            readme.contains(phrase),
            "file integrity demo omits {phrase}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let board = root.join("demos/single/file-id-integrity/.pinto");
    assert!(board.join("tasks/T-1.md").is_file());
    assert!(board.join("tasks/T-3.md").is_file());
    assert!(board.join("archive/T-2.md").is_file());
    assert!(board.join("sprints/S-1.md").is_file());
}

#[test]
fn parse_error_classification_demo_contains_reproducible_board_and_exit_code_guide() {
    let readme = repository_file("demos/single/parse-error-classification/README.md");
    for phrase in [
        "Error::Parse",
        "Error::MissingFrontmatter",
        "invalid TOML",
        "remove a required section",
        "cargo run --manifest-path ../../../Cargo.toml -- board",
        "cargo run --manifest-path ../../../Cargo.toml -- list",
        "exits with code 1",
        "exit code 2",
    ] {
        assert!(readme.contains(phrase), "parse error demo omits {phrase}");
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let board = root.join("demos/single/parse-error-classification/.pinto");
    assert!(board.join("config.toml").is_file());
    assert!(board.join("tasks/T-1.md").is_file());
}

#[test]
fn sprint_edit_delete_demo_contains_recovery_and_assignment_data() {
    let readme = repository_file("demos/single/sprint-edit-delete/README.md");
    for phrase in [
        "sprint edit",
        "sprint start",
        "sprint remove",
        "clearing their `sprint` assignment",
        "cargo run --manifest-path ../../../Cargo.toml -- sprint list --json",
        "cargo run --manifest-path ../../../Cargo.toml -- show T-2 --json",
    ] {
        assert!(
            readme.contains(phrase),
            "sprint edit/delete demo omits {phrase}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let board = root.join("demos/single/sprint-edit-delete/.pinto");
    assert!(board.join("sprints/S-1.md").is_file());
    assert!(!board.join("sprints/S-2.md").exists());
    assert!(board.join("tasks/T-1.md").is_file());
    assert!(board.join("tasks/T-2.md").is_file());
}

#[test]
fn sprint_closed_assignment_demo_contains_state_rule_and_recovery_data() {
    let readme = repository_file("demos/single/sprint-closed-assignment/README.md");
    for phrase in [
        "sprint list --json",
        "sprint add S-1 T-2",
        "sprint unassign S-1 T-1",
        "exits 1",
        "the other remains unassigned",
        "assignment made before closure",
    ] {
        assert!(
            readme.contains(phrase),
            "closed assignment demo omits {phrase}"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let board = root.join("demos/single/sprint-closed-assignment/.pinto");
    assert!(board.join("sprints/S-1.md").is_file());
    assert!(board.join("tasks/T-1.md").is_file());
    assert!(board.join("tasks/T-2.md").is_file());
}

fn collect_rust_sources(directory: &Path, files: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("read Rust source directory") {
        let entry = entry.expect("read Rust source entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(fs::read_to_string(path).expect("read Rust source file"));
        }
    }
}

#[test]
fn rust_documentation_uses_clear_english() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    collect_rust_sources(&root.join("src"), &mut sources);
    let corpus = sources.join("\n");

    for phrase in [
        "Get exclusive.",
        "Set the waiting limit",
        "keep circulation on alert",
        "Specified itself as the sorting criterion",
        "The content edited with the editor is invalid and cannot be reflected in PBI.",
        "Archive archive.",
        "It disappears from tasks.",
        "The operation can be continued while keeping the error",
        "The same time is stabilized by the ID string.",
    ] {
        assert!(
            !corpus.contains(phrase),
            "Rust documentation still contains translation-like wording: {phrase}"
        );
    }
}

#[test]
fn ci_msrv_job_covers_all_features_and_sqlite() {
    let workflow = repository_file(".github/workflows/ci.yml");
    let msrv_job = workflow
        .split_once("  check:")
        .map(|(job, _)| job)
        .expect("workflow contains a check job after the MSRV job");

    for command in [
        "cargo build --locked",
        "cargo test --locked",
        "cargo build --all-features --locked",
        "cargo test --all-features --locked",
    ] {
        assert!(
            msrv_job.contains(command),
            "MSRV job omits the required command: {command}"
        );
    }
}

#[test]
fn local_release_gate_runs_every_required_quality_job() {
    let mise = repository_file("mise.toml");
    let gate = mise
        .split_once("[tasks.release-check]")
        .map(|(_, rest)| rest)
        .expect("mise.toml defines the release-check task");
    for task in ["check", "coverage", "audit", "deny"] {
        assert!(gate.contains(task), "release-check omits the {task} task");
    }
}

#[test]
fn ci_runs_on_primary_development_branch_pushes() {
    let workflow = repository_file(".github/workflows/ci.yml");
    assert!(
        workflow.contains("branches: [main, develop]"),
        "CI push trigger must include both main and develop"
    );
}

#[test]
fn coverage_gate_checks_the_uploaded_cobertura_metric() {
    let mise = repository_file("mise.toml");
    assert!(mise.contains("--cobertura --output-path coverage.xml"));
    assert!(mise.contains("./scripts/check-coverage.sh coverage.xml 0.95"));
    assert!(!mise.contains("--fail-under-lines"));

    let checker = repository_file("scripts/check-coverage.sh");
    for marker in ["line-rate", "minimum", "coverage.xml"] {
        assert!(checker.contains(marker), "coverage checker omits {marker}");
    }

    let workflow = repository_file(".github/workflows/ci.yml");
    let coverage_job = workflow
        .split_once("  coverage:")
        .and_then(|(_, rest)| rest.split_once("  dependency-policy:"))
        .map(|(job, _)| job)
        .expect("workflow contains a bounded coverage job");
    assert!(coverage_job.contains("if: success()"));
    assert!(!coverage_job.contains("if: always()"));
}
