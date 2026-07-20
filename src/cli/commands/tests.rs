use super::automation::{
    AutomationExecution, ValidatedAutomationCommand, automation_command_name,
    automation_execution_result, automation_target_ids, first_item_id_in_output, parsed_item_id,
    read_automation_plan,
};
use super::item::{combine_template_body, report_failures};
use super::sprint::cmd_sprint;
use crate::cli::args::{Cli, SprintArgs, SprintCommand};
use clap::CommandFactory;
use pinto::automation::AutomationPlan;
use pinto::backlog::ItemId;
use pinto::error::Error;
use std::path::Path;

fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn automation_names_and_target_ids_cover_command_shapes() {
    assert_eq!(automation_command_name(&argv(&["dep", "add"])), "dep add");
    assert_eq!(
        automation_command_name(&argv(&["link", "sync"])),
        "link sync"
    );
    assert_eq!(
        automation_command_name(&argv(&["sprint", "new"])),
        "sprint new"
    );
    assert_eq!(automation_command_name(&argv(&["list"])), "list");
    assert_eq!(automation_command_name(&[]), "unknown");

    assert!(automation_target_ids(&[]).is_empty());
    assert_eq!(
        automation_target_ids(&argv(&["move", "T-1", "invalid", "T-2"])),
        ["T-1", "T-2"]
    );
    assert_eq!(automation_target_ids(&argv(&["edit", "T-3"])), ["T-3"]);
    assert_eq!(
        automation_target_ids(&argv(&["reorder", "T-4", "--top"])),
        ["T-4"]
    );
    assert_eq!(
        automation_target_ids(&argv(&["remove", "T-5", "T-6"])),
        ["T-5", "T-6"]
    );
    assert_eq!(
        automation_target_ids(&argv(&["dep", "add", "T-7", "T-8"])),
        ["T-7"]
    );
    assert_eq!(
        automation_target_ids(&argv(&["link", "add", "T-9", "abc"])),
        ["T-9"]
    );
    assert_eq!(
        automation_target_ids(&argv(&["sprint", "add", "S-1", "T-10"])),
        ["T-10"]
    );
    assert!(automation_target_ids(&argv(&["unknown", "T-11"])).is_empty());
}

#[test]
fn automation_schema_tracks_safe_cli_commands_and_aliases() {
    let schema = AutomationPlan::json_schema();
    let names = schema["$defs"]["command"]["prefixItems"][0]["enum"]
        .as_array()
        .expect("schema command names are an array");
    let is_unsafe = |name: &str| {
        matches!(
            name,
            "automate" | "auto" | "shell" | "kanban" | "k" | "completion"
        )
    };

    for command in Cli::command().get_subcommands() {
        if command.get_name() == "help" {
            continue;
        }
        let command_names =
            std::iter::once(command.get_name()).chain(command.get_visible_aliases());
        for name in command_names {
            if is_unsafe(name) {
                assert!(!names.iter().any(|value| value == name));
                assert!(
                    AutomationPlan::parse(&format!(r#"{{"commands":[["{name}"]]}}"#)).is_err(),
                    "unsafe command must be rejected: {name}"
                );
            } else {
                assert!(
                    names.iter().any(|value| value == name),
                    "safe CLI command is missing from the schema: {name}"
                );
            }
        }
    }
}

#[test]
fn automation_results_extract_created_ids_and_sanitize_errors() {
    let command = ValidatedAutomationCommand {
        index: 1,
        argv: argv(&["add", "Task"]),
        name: "add".to_string(),
        error: None,
    };
    let created = automation_execution_result(
        &command,
        &AutomationExecution {
            success: true,
            exit_code: Some(0),
            stdout: "Created T-42: Task".to_string(),
            stderr: String::new(),
        },
        "succeeded",
    );
    assert_eq!(created.created_ids, ["T-42"]);
    assert_eq!(created.error, None);

    for (exit_code, expected) in [
        (Some(1), "command exited with status 1"),
        (None, "command exited with status unknown"),
    ] {
        let failed = automation_execution_result(
            &command,
            &AutomationExecution {
                success: false,
                exit_code,
                stdout: String::new(),
                stderr: String::new(),
            },
            "failed",
        );
        assert_eq!(failed.error.as_deref(), Some(expected));
    }

    let stderr = automation_execution_result(
        &command,
        &AutomationExecution {
            success: false,
            exit_code: Some(1),
            stdout: String::new(),
            stderr: "  user-facing failure\n".to_string(),
        },
        "failed",
    );
    assert_eq!(stderr.error.as_deref(), Some("user-facing failure"));
    assert_eq!(
        first_item_id_in_output("created: [T-1], next T-2"),
        Some("T-1".to_string())
    );
    assert_eq!(parsed_item_id(Some(&"bad".to_string())), None);
}

#[test]
fn template_body_and_failure_reporting_keep_edge_cases_explicit() {
    assert_eq!(
        combine_template_body(String::new(), "body".to_string()),
        "body"
    );
    assert_eq!(
        combine_template_body("template".to_string(), String::new()),
        "template"
    );
    assert_eq!(
        combine_template_body("template\n".to_string(), "body".to_string()),
        "template\n\nbody"
    );
    assert_eq!(
        combine_template_body("template".to_string(), "body".to_string()),
        "template\n\nbody"
    );

    let item_id = "T-1".parse::<ItemId>().expect("valid item id");
    let result = report_failures(
        vec![
            ("T-1".to_string(), Error::NotFound(item_id)),
            (
                "T-2".to_string(),
                Error::Io {
                    path: "/tmp/pinto-test".into(),
                    message: "read failed".to_string(),
                },
            ),
        ],
        "move",
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn sprint_capacity_rejects_an_incomplete_programmatic_argument_set() {
    let error = cmd_sprint(SprintArgs {
        command: SprintCommand::Capacity {
            id: "S-1".to_string(),
            daily_hours: Some(8.0),
            holidays: None,
            deduction_factor: None,
            json: false,
        },
    })
    .await
    .expect_err("partial capacity settings must be rejected");

    assert!(error.to_string().contains("must be provided together"));
}

#[tokio::test]
async fn automation_plan_source_handles_inline_and_invalid_sources() {
    let inline = "{\0\"commands\":[]}";
    assert_eq!(read_automation_plan(inline).await.unwrap(), inline);

    let invalid_path = "automation\0plan.json";
    let error = read_automation_plan(invalid_path)
        .await
        .expect_err("invalid path should return a structured source error");
    assert!(matches!(
        error.downcast_ref::<Error>(),
        Some(Error::AutomationPlanSource { path, .. })
            if path == Path::new(invalid_path)
    ));

    let directory = tempfile::tempdir().expect("temporary directory");
    let directory_path = directory.path().to_str().expect("temporary path is UTF-8");
    let error = read_automation_plan(directory_path)
        .await
        .expect_err("a directory is not a readable plan file");
    assert!(matches!(
        error.downcast_ref::<Error>(),
        Some(Error::AutomationPlanSource { path, .. })
            if path == directory.path()
    ));

    let missing_path = directory.path().join("missing.json");
    let missing_path = missing_path.to_str().expect("temporary path is UTF-8");
    let error = read_automation_plan(missing_path)
        .await
        .expect_err("missing path should return a structured source error");
    assert!(matches!(
        error.downcast_ref::<Error>(),
        Some(Error::AutomationPlanSource { path, message })
            if path == Path::new(missing_path) && message == "file does not exist"
    ));
}
