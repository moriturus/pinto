use super::super::*;

#[test]
fn default_scope_keeps_every_workflow_column() {
    let workflow = ["backlog", "ready", "in-progress", "review", "done"].map(str::to_string);

    let visible = resolve_display_columns(&workflow, &[], None).expect("scope resolves");

    assert_eq!(visible, workflow);
}

#[test]
fn hidden_columns_are_removed_in_workflow_order() {
    let workflow = ["backlog", "ready", "in-progress", "review", "done"].map(str::to_string);

    let visible =
        resolve_display_columns(&workflow, &["backlog".to_string()], None).expect("scope resolves");

    assert_eq!(visible, ["ready", "in-progress", "review", "done"]);
}

#[test]
fn explicit_columns_override_hidden_columns_and_keep_workflow_order() {
    let workflow = ["backlog", "ready", "in-progress", "review", "done"].map(str::to_string);
    let requested = ["done".to_string(), "backlog".to_string()];

    let visible = resolve_display_columns(&workflow, &["backlog".to_string()], Some(&requested))
        .expect("scope resolves");

    assert_eq!(visible, ["backlog", "done"]);
}

#[test]
fn explicit_unknown_column_is_rejected_before_terminal_startup() {
    let workflow = ["todo".to_string(), "done".to_string()];

    let error = resolve_display_columns(
        &workflow,
        &[],
        Some(&["todo".to_string(), "missing".to_string()]),
    )
    .expect_err("unknown column rejected");

    assert!(error.to_string().contains("missing"));
}
