//! Integration checks for the persisted demo boards.
//!
//! Every demo receives the same read-only smoke checks. Scenarios that intentionally
//! demonstrate user errors use isolated copies so the checked-in fixtures remain unchanged.

use assert_cmd::Command;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::{TempDir, tempdir};

const INTENTIONAL_ERROR_DEMOS: &[&str] = &[
    "single/config-validation",
    "single/parse-error-classification",
    "single/i18n",
    "single/move-multiple",
    "single/remove-multiple",
    "single/remove-force-safety",
    "single/sprint-assignment-validation",
    "single/sprint-bulk-assignment",
    "single/sprint-closed-assignment",
];

#[derive(Debug)]
struct Demo {
    name: String,
    path: PathBuf,
}

fn discovered_demos() -> Vec<Demo> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("demos");
    let mut demos = Vec::new();

    for group in ["single", "combined"] {
        let group_dir = root.join(group);
        let entries = fs::read_dir(&group_dir)
            .unwrap_or_else(|error| panic!("read demo group {group}: {error}"));
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| panic!("read demo entry in {group}: {error}"));
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let name = format!("{group}/{}", entry.file_name().to_string_lossy());
            assert!(
                path.join("README.md").is_file(),
                "demo {name} must include README.md"
            );
            assert!(
                path.join(".pinto").is_dir(),
                "demo {name} must include a .pinto board"
            );
            demos.push(Demo { name, path });
        }
    }

    demos.sort_by(|left, right| left.name.cmp(&right.name));
    assert!(
        !demos.is_empty(),
        "at least one demo board must be discovered"
    );
    demos
}

fn run_pinto(dir: &Path, args: &[&str]) -> Output {
    let mut command = Command::cargo_bin("pinto").expect("pinto binary builds");
    command
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("run pinto {args:?} in {}: {error}", dir.display()))
}

fn run_pinto_with_env(dir: &Path, args: &[&str], key: &str, value: &str) -> Output {
    let mut command = Command::cargo_bin("pinto").expect("pinto binary builds");
    command
        .current_dir(dir)
        .args(args)
        .env(key, value)
        .output()
        .unwrap_or_else(|error| panic!("run pinto {args:?} in {}: {error}", dir.display()))
}

fn diagnostics(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_success(name: &str, args: &[&str], output: &Output) {
    assert!(
        output.status.success(),
        "{name} {args:?} failed:\n{}",
        diagnostics(output)
    );
}

fn assert_user_error(name: &str, args: &[&str], output: &Output, marker: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{name} {args:?} must exit with user-error code 1:\n{}",
        diagnostics(output)
    );
    assert!(
        diagnostics(output).contains(marker),
        "{name} {args:?} must mention {marker:?}:\n{}",
        diagnostics(output)
    );
}

fn json_output(name: &str, dir: &Path, args: &[&str]) -> Value {
    let output = run_pinto(dir, args);
    assert_success(name, args, &output);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{name} {args:?} returned invalid JSON ({error}):\n{}",
            diagnostics(&output)
        )
    })
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn copy_demo(demo: &Demo, temp: &TempDir) -> PathBuf {
    let destination = temp.path().join(demo.name.replace(['/', '\\'], "__"));
    copy_tree(&demo.path, &destination)
        .unwrap_or_else(|error| panic!("copy demo {}: {error}", demo.name));
    destination
}

fn documents_exit_code_contract(path: &Path) -> bool {
    let readme = fs::read_to_string(path.join("README.md"))
        .unwrap_or_else(|error| panic!("read demo README {}: {error}", path.display()));
    [
        "exits with code",
        "exit code",
        "exits with status",
        "exits 1",
    ]
    .iter()
    .any(|marker| readme.contains(marker))
}

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap_or_else(|error| {
            panic!("read snapshot directory {}: {error}", current.display())
        }) {
            let entry = entry.unwrap_or_else(|error| panic!("read snapshot entry: {error}"));
            let path = entry.path();
            if entry
                .file_type()
                .unwrap_or_else(|error| panic!("read file type for {}: {error}", path.display()))
                .is_dir()
            {
                collect(root, &path, files);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or_else(|error| panic!("snapshot path prefix: {error}"))
                    .to_path_buf();
                let contents = fs::read(&path).unwrap_or_else(|error| {
                    panic!("read snapshot file {}: {error}", path.display())
                });
                files.insert(relative, contents);
            }
        }
    }

    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

#[test]
fn every_persisted_demo_board_passes_read_only_cli_smoke_checks() {
    for demo in discovered_demos() {
        let before = snapshot_tree(&demo.path.join(".pinto"));

        let list_args = ["list", "--json"];
        let list = run_pinto(&demo.path, &list_args);
        assert_success(&demo.name, &list_args, &list);
        let items: Vec<Value> = serde_json::from_slice(&list.stdout).unwrap_or_else(|error| {
            panic!(
                "{} list returned invalid JSON ({error}):\n{}",
                demo.name,
                diagnostics(&list)
            )
        });

        let board_args = ["board", "--json"];
        assert_success(&demo.name, &board_args, &run_pinto(&demo.path, &board_args));
        let roots_args = ["list", "--roots-only", "--json"];
        assert_success(&demo.name, &roots_args, &run_pinto(&demo.path, &roots_args));
        let root_board_args = ["board", "--roots-only", "--json"];
        assert_success(
            &demo.name,
            &root_board_args,
            &run_pinto(&demo.path, &root_board_args),
        );
        let sprints_args = ["sprint", "list", "--json"];
        assert_success(
            &demo.name,
            &sprints_args,
            &run_pinto(&demo.path, &sprints_args),
        );

        if let Some(id) = items
            .first()
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
        {
            let show_args = ["show", id, "--json"];
            assert_success(&demo.name, &show_args, &run_pinto(&demo.path, &show_args));
        }

        let after = snapshot_tree(&demo.path.join(".pinto"));
        assert_eq!(
            before, after,
            "read-only smoke commands modified demo {}",
            demo.name
        );
    }
}

#[test]
fn sprint_lifecycle_demo_persists_rollover_and_separate_spillover() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let demo = root.join("demos/single/sprint-lifecycle");
    let sprints = json_output(
        "single/sprint-lifecycle",
        &demo,
        &["sprint", "list", "--json"],
    );
    let source = sprints
        .as_array()
        .expect("sprint list is an array")
        .iter()
        .find(|sprint| sprint["id"] == "S-2")
        .expect("S-2 exists");
    assert_eq!(source["state"], "closed");
    assert_eq!(source["spillover_points"], 8);
    assert_eq!(source["spillover_items"], 2);
    assert_eq!(source["unestimated_spillover_items"], 0);

    let target_items = json_output(
        "single/sprint-lifecycle",
        &demo,
        &["list", "--sprint", "S-3", "--json"],
    );
    let ids = target_items
        .as_array()
        .expect("item list is an array")
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"T-3"));
    assert!(ids.contains(&"T-5"));
}

#[test]
fn intentional_error_demos_are_registered_and_keep_user_error_contracts() {
    let demos = discovered_demos();
    for name in INTENTIONAL_ERROR_DEMOS {
        assert!(
            demos.iter().any(|demo| demo.name == *name),
            "intentional-error demo {name} is not present in the discovered board set"
        );
    }
    for demo in &demos {
        if documents_exit_code_contract(&demo.path) {
            assert!(
                INTENTIONAL_ERROR_DEMOS
                    .iter()
                    .any(|registered| *registered == demo.name),
                "demo {} documents an exit-code contract but is missing from intentional-error coverage",
                demo.name
            );
        }
    }

    let temp = tempdir().expect("create demo error fixture directory");
    let find_demo = |name: &str| {
        demos
            .iter()
            .find(|demo| demo.name == name)
            .unwrap_or_else(|| panic!("demo {name} is not registered"))
    };

    let config_demo = find_demo("single/config-validation");
    let config_board = copy_demo(config_demo, &temp);
    let config_path = config_board.join(".pinto/config.toml");
    let config = fs::read_to_string(&config_path).expect("read copied config");
    let invalid_config =
        config.replacen("markdown = true", "markdown = true\ntimezome = \"UTC\"", 1);
    assert_ne!(
        config, invalid_config,
        "config fixture must contain markdown setting"
    );
    fs::write(&config_path, invalid_config).expect("write invalid copied config");
    let config_args = ["board"];
    assert_user_error(
        &config_demo.name,
        &config_args,
        &run_pinto(&config_board, &config_args),
        "timezome",
    );

    let parse_demo = find_demo("single/parse-error-classification");
    let invalid_config_board = copy_demo(parse_demo, &temp);
    fs::write(
        invalid_config_board.join(".pinto/config.toml"),
        "columns = = broken\n",
    )
    .expect("write malformed copied config");
    let invalid_config_args = ["board"];
    assert_user_error(
        &parse_demo.name,
        &invalid_config_args,
        &run_pinto(&invalid_config_board, &invalid_config_args),
        "config.toml",
    );

    let invalid_task_board = copy_demo(parse_demo, &temp);
    fs::write(
        invalid_task_board.join(".pinto/tasks/T-1.md"),
        "not frontmatter\n",
    )
    .expect("write malformed copied task");
    let invalid_task_args = ["list"];
    assert_user_error(
        &parse_demo.name,
        &invalid_task_args,
        &run_pinto(&invalid_task_board, &invalid_task_args),
        "T-1.md",
    );

    let move_demo = find_demo("single/move-multiple");
    let move_board = copy_demo(move_demo, &temp);
    let move_args = ["move", "T-404", "T-1", "done"];
    assert_user_error(
        &move_demo.name,
        &move_args,
        &run_pinto(&move_board, &move_args),
        "T-404",
    );
    let moved = json_output(&move_demo.name, &move_board, &["show", "T-1", "--json"]);
    assert_eq!(
        moved[0]["status"], "done",
        "valid move must survive partial failure"
    );

    let remove_demo = find_demo("single/remove-multiple");
    let remove_board = copy_demo(remove_demo, &temp);
    let remove_args = ["rm", "T-404", "T-1"];
    assert_user_error(
        &remove_demo.name,
        &remove_args,
        &run_pinto(&remove_board, &remove_args),
        "T-404",
    );
    let remaining = json_output(&remove_demo.name, &remove_board, &["list", "--json"]);
    assert!(
        !remaining
            .as_array()
            .expect("list JSON is an array")
            .iter()
            .any(|item| item["id"] == "T-1"),
        "valid archive must survive partial failure"
    );

    let force_demo = find_demo("single/remove-force-safety");
    let force_board = copy_demo(force_demo, &temp);
    let force_args = ["rm", "T-1", "--force"];
    let force_output = run_pinto(&force_board, &force_args);
    assert_user_error(&force_demo.name, &force_args, &force_output, "T-2");
    assert!(
        diagnostics(&force_output).contains("T-3"),
        "force removal diagnostic must include every reverse reference:\n{}",
        diagnostics(&force_output)
    );

    let assignment_demo = find_demo("single/sprint-assignment-validation");
    let assignment_board = copy_demo(assignment_demo, &temp);
    let before_assignment = json_output(
        &assignment_demo.name,
        &assignment_board,
        &["list", "--json"],
    );
    let invalid_add = ["add", "Missing Sprint", "--sprint", "S-404"];
    assert_user_error(
        &assignment_demo.name,
        &invalid_add,
        &run_pinto(&assignment_board, &invalid_add),
        "S-404",
    );
    let invalid_edit = ["edit", "T-2", "--sprint", "S 1"];
    assert_user_error(
        &assignment_demo.name,
        &invalid_edit,
        &run_pinto(&assignment_board, &invalid_edit),
        "S 1",
    );
    let invalid_sprint_add = ["sprint", "add", "S 1", "T-2"];
    assert_user_error(
        &assignment_demo.name,
        &invalid_sprint_add,
        &run_pinto(&assignment_board, &invalid_sprint_add),
        "S 1",
    );
    let after_assignment = json_output(
        &assignment_demo.name,
        &assignment_board,
        &["list", "--json"],
    );
    assert_eq!(
        before_assignment, after_assignment,
        "failed Sprint assignments must not mutate the copied board"
    );

    let closed_demo = find_demo("single/sprint-closed-assignment");
    let closed_board = copy_demo(closed_demo, &temp);
    let closed_args = ["sprint", "add", "S-1", "T-2"];
    assert_user_error(
        &closed_demo.name,
        &closed_args,
        &run_pinto(&closed_board, &closed_args),
        "closed sprint",
    );

    let bulk_demo = find_demo("single/sprint-bulk-assignment");
    let bulk_board = copy_demo(bulk_demo, &temp);
    let before_bulk = json_output(&bulk_demo.name, &bulk_board, &["list", "--json"]);
    let bulk_args = ["sprint", "add", "S-1", "--status", "review"];
    assert_user_error(
        &bulk_demo.name,
        &bulk_args,
        &run_pinto(&bulk_board, &bulk_args),
        "already assigned",
    );
    let after_bulk = json_output(&bulk_demo.name, &bulk_board, &["list", "--json"]);
    assert_eq!(
        before_bulk, after_bulk,
        "failed bulk assignment must be atomic"
    );

    let i18n_demo = find_demo("single/i18n");
    let i18n_board = copy_demo(i18n_demo, &temp);
    let i18n_args = ["add", ""];
    assert_user_error(
        &i18n_demo.name,
        &i18n_args,
        &run_pinto_with_env(&i18n_board, &i18n_args, "LC_ALL", "ja_JP.UTF-8"),
        "タイトル",
    );
}
