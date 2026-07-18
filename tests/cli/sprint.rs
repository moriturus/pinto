//! Sprint and cycle-time commands.

use super::common::*;

#[test]
fn sprint_new_uses_a_plain_text_sprint_template() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let template_dir = dir.path().join(".pinto/templates/sprint");
    std::fs::create_dir_all(&template_dir).expect("create template dir");
    std::fs::write(
        template_dir.join("planning.md"),
        "## Sprint goal\n\nShip it",
    )
    .expect("write template");

    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--template",
            "planning",
        ])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    assert_eq!(value[0]["goal"], "## Sprint goal\n\nShip it");
}

#[test]
fn cycletime_reports_cycle_and_lead_for_completed_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    // Move both items to done; start_at and done_at are recorded.
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "done"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("cycletime")
        .assert()
        .success()
        .stdout(predicate::str::contains("2 completed"))
        .stdout(predicate::str::contains("cycle (start → done)"))
        .stdout(predicate::str::contains("lead  (created → done)"));
}

#[test]
fn ct_alias_emits_json_summaries() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["ct", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"completed\": 1"))
        .stdout(predicate::str::contains("\"cycle\""))
        .stdout(predicate::str::contains("\"lead\""))
        .stdout(predicate::str::contains("\"mean_seconds\""))
        .stdout(predicate::str::contains("\"missing_start\""));
}

#[test]
fn cycletime_reports_zero_when_nothing_completed() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();

    pinto(dir.path())
        .arg("cycletime")
        .assert()
        .success()
        .stdout(predicate::str::contains("0 completed"))
        .stdout(predicate::str::contains("cycle (").not());
}

#[test]
fn cycletime_filters_by_sprint() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "In sprint"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Out of sprint"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "done"])
        .assert()
        .success();

    // Only the one item assigned to S-1 is included.
    pinto(dir.path())
        .args(["cycletime", "--sprint", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 completed"));
}

#[test]
fn cycletime_filters_out_items_completed_before_the_since_bound() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();

    // Completion is based on the current time, so a far-future lower bound yields zero matches.
    pinto(dir.path())
        .args(["cycletime", "--since", "2999-01-01"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 completed"));
}

#[test]
fn cycletime_without_init_errors_and_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("cycletime")
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn sprint_without_init_errors_and_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .args(["sprint", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn sprint_list_json_outputs_sprint_schema() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Planning", "--goal", "Ship MVP"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    let sprints = value.as_array().expect("sprint list --json is an array");
    assert_eq!(sprints.len(), 1);
    let sprint = &sprints[0];
    assert_eq!(sprint["id"], "S-1");
    assert_eq!(sprint["title"], "Planning");
    assert_eq!(sprint["state"], "planned");
    assert_eq!(sprint["goal"], "Ship MVP");
    assert_eq!(sprint["start"], serde_json::Value::Null);
    assert_eq!(sprint["end"], serde_json::Value::Null);
    assert_eq!(sprint["closed_at"], serde_json::Value::Null);
    assert_eq!(sprint["spillover_points"], 0);
    assert_eq!(sprint["spillover_items"], 0);
    assert_eq!(sprint["unestimated_spillover_items"], 0);
    assert!(sprint["created"].is_string(), "created is RFC3339 string");
}

#[test]
fn sprint_new_creates_sprint_file_with_frontmatter() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--goal",
            "Ship the MVP",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created sprint S-1"))
        .stdout(predicate::str::contains("Sprint One"));

    let file = std::fs::read_to_string(dir.path().join(".pinto/sprints/S-1.md"))
        .expect("sprint file exists");
    assert!(file.contains("id = \"S-1\""), "frontmatter carries id");
    assert!(
        file.contains("state = \"planned\""),
        "new sprint is planned"
    );
    assert!(
        file.contains("title = \"Sprint One\""),
        "title stored in frontmatter"
    );
    assert!(file.contains("\n\nShip the MVP\n"), "goal stored as body");
}

#[test]
fn sprint_new_with_date_only_period_shows_midnight_in_list() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("timezone = \"local\"", "timezone = \"UTC\""),
    )
    .expect("configure UTC display");

    // A date-only value is interpreted as 00:00 UTC, and the display timezone is fixed to UTC.
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-20",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "2026-07-06 00:00 → 2026-07-20 00:00",
        ));
}

#[test]
fn sprint_new_with_minute_precision_period_is_shown_in_list() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("timezone = \"local\"", "timezone = \"UTC\""),
    )
    .expect("configure UTC display");

    // Handle minute-level timestamps and show them unchanged in the UTC list.
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06T09:30",
            "--end",
            "2026-07-20T18:15",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "2026-07-06 09:30 → 2026-07-20 18:15",
        ));
}

#[test]
fn sprint_start_requires_a_goal_but_creation_does_not() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "sprint goal must be set before starting the sprint",
        ));

    pinto(dir.path())
        .args(["sprint", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"state\": \"planned\""));
}

#[test]
fn sprint_edit_updates_goal_title_and_period_then_allows_start() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Planning"])
        .assert()
        .success();

    pinto(dir.path())
        .args([
            "sprint",
            "edit",
            "S-1",
            "--title",
            "Execution",
            "--goal",
            "Ship the sprint",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-10",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated sprint S-1"));

    let edited = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    assert_eq!(edited[0]["title"], "Execution");
    assert_eq!(edited[0]["goal"], "Ship the sprint");
    assert_eq!(edited[0]["start"], "2026-07-06T00:00:00+00:00");
    assert_eq!(edited[0]["end"], "2026-07-10T00:00:00+00:00");

    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Started sprint S-1"));
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]))[0]["state"],
        "active"
    );
}

#[test]
fn sprint_edit_rejects_empty_title_and_no_fields_without_mutation() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Planning"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "edit", "S-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no fields to update"));
    pinto(dir.path())
        .args(["sprint", "edit", "S-1", "--title", ""])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("sprint title must not be empty"));

    let unchanged = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    assert_eq!(unchanged[0]["title"], "Planning");
    assert_eq!(unchanged[0]["goal"], "");
}

#[test]
fn sprint_capacity_sets_and_displays_calculated_hours() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-10",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args([
            "sprint",
            "capacity",
            "S-1",
            "--daily-hours",
            "8",
            "--holidays",
            "1",
            "--deduction-factor",
            "0.8",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Working days: 4"))
        .stdout(predicate::str::contains("Capacity: 25.6 hours"));

    let value = json_stdout(pinto(dir.path()).args(["sprint", "capacity", "S-1", "--json"]));
    assert_eq!(value["working_days"], 4);
    assert_eq!(value["hours"], 25.6);
}

#[test]
fn sprint_capacity_rejects_out_of_range_deduction_factor() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-10",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args([
            "sprint",
            "capacity",
            "S-1",
            "--daily-hours",
            "8",
            "--holidays",
            "0",
            "--deduction-factor",
            "1.1",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "deduction factor must be a finite number from 0 to 1",
        ));
}

#[test]
fn sprint_start_warns_when_capacity_is_exceeded_without_blocking() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--goal",
            "Ship it",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-06",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Large task", "--points", "5"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "sprint",
            "capacity",
            "S-1",
            "--daily-hours",
            "4",
            "--holidays",
            "0",
            "--deduction-factor",
            "1",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Started sprint S-1"))
        .stderr(predicate::str::contains("capacity threshold"));
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]))[0]["state"],
        "active"
    );
}

#[test]
fn sprint_add_warns_when_velocity_is_exceeded_and_assigns_all_matching_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "History", "--goal", "Ship it"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Completed history", "--points", "3"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "new", "S-2", "Current", "--goal", "Ship it"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "First current task", "--points", "2"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Second current task", "--points", "2"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "add", "S-2", "--status", "todo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Assigned T-2 to sprint S-2"))
        .stdout(predicate::str::contains("Assigned T-3 to sprint S-2"))
        .stderr(predicate::str::contains("velocity threshold"));

    let assigned = json_stdout(pinto(dir.path()).args(["list", "--sprint", "S-2", "--json"]));
    assert_eq!(assigned.as_array().expect("list JSON array").len(), 2);
}

#[test]
fn sprint_add_without_a_comparison_is_silent_and_still_assigns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Task", "--points", "8"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("threshold").not());
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-1", "--json"]))["sprint"],
        "S-1"
    );
}

#[test]
fn sprint_new_start_without_end_is_usage_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // `--start` and `--end` must be provided together; specifying only one is invalid.
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06",
        ])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn sprint_new_start_after_end_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-20",
            "--end",
            "2026-07-06",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid sprint period"));
}

#[test]
fn sprint_new_duplicate_id_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "First"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Second"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn sprint_start_then_close_transitions_state() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--goal",
            "Ship the sprint",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Started sprint S-1"));
    assert!(
        std::fs::read_to_string(dir.path().join(".pinto/sprints/S-1.md"))
            .unwrap()
            .contains("state = \"active\""),
        "start moves to active"
    );

    pinto(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Closed sprint S-1"));
    assert!(
        std::fs::read_to_string(dir.path().join(".pinto/sprints/S-1.md"))
            .unwrap()
            .contains("state = \"closed\""),
        "close moves to closed"
    );
}

fn init_active_sprint_with_unfinished_items(dir: &Path) {
    pinto(dir).arg("init").assert().success();
    pinto(dir)
        .args([
            "sprint",
            "new",
            "S-1",
            "Source",
            "--goal",
            "Ship the sprint",
        ])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "new", "S-2", "Target"])
        .assert()
        .success();
    pinto(dir)
        .args(["add", "Completed", "--points", "3", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir)
        .args([
            "add",
            "Estimated unfinished",
            "--points",
            "5",
            "--sprint",
            "S-1",
        ])
        .assert()
        .success();
    pinto(dir)
        .args(["add", "Unestimated unfinished", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir).args(["move", "T-1", "done"]).assert().success();
    pinto(dir)
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();
}

#[test]
fn sprint_close_rollover_moves_only_unfinished_items_and_keeps_velocity_done_only() {
    let dir = TempDir::new().expect("temp dir");
    init_active_sprint_with_unfinished_items(dir.path());
    let completed_before = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));

    pinto(dir.path())
        .args(["sprint", "close", "S-1", "--rollover", "S-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Closed sprint S-1"));

    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-1", "--json"])),
        completed_before,
        "completed PBIs are not rewritten"
    );
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-2", "--json"]))["sprint"],
        "S-2"
    );
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-3", "--json"]))["sprint"],
        "S-2"
    );

    let sprints = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    let source = sprints
        .as_array()
        .expect("sprint list is an array")
        .iter()
        .find(|sprint| sprint["id"] == "S-1")
        .expect("source sprint");
    assert_eq!(source["state"], "closed");
    assert!(source["closed_at"].is_string());
    assert_eq!(source["spillover_points"], 5);
    assert_eq!(source["spillover_items"], 2);
    assert_eq!(source["unestimated_spillover_items"], 1);

    pinto(dir.path())
        .args(["sprint", "velocity", "--recent", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("S-1  3 points"))
        .stdout(predicate::str::contains(
            "spillover: 5 points (2 items, 1 unestimated)",
        ))
        .stdout(predicate::str::contains("Average: 1.5 points"));
}

#[test]
fn sprint_close_release_unassigns_only_unfinished_items() {
    let dir = TempDir::new().expect("temp dir");
    init_active_sprint_with_unfinished_items(dir.path());
    let completed_before = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));

    pinto(dir.path())
        .args(["sprint", "close", "S-1", "--release"])
        .assert()
        .success();

    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-1", "--json"])),
        completed_before
    );
    assert!(show_json(pinto(dir.path()).args(["show", "T-2", "--json"]))["sprint"].is_null());
    assert!(show_json(pinto(dir.path()).args(["show", "T-3", "--json"]))["sprint"].is_null());

    let sprints = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    assert_eq!(sprints[0]["spillover_points"], 5);
    assert_eq!(sprints[0]["spillover_items"], 2);
    assert_eq!(sprints[0]["unestimated_spillover_items"], 1);
}

#[test]
fn sprint_close_retained_spillover_completed_later_does_not_change_velocity() {
    let dir = TempDir::new().expect("temp dir");
    init_active_sprint_with_unfinished_items(dir.path());
    pinto(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["move", "T-2", "done"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "velocity", "--recent", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("S-1  3 points"))
        .stdout(predicate::str::contains(
            "spillover: 5 points (2 items, 1 unestimated)",
        ))
        .stdout(predicate::str::contains("Average: 1.5 points"));
}

#[test]
fn sprint_close_rejects_invalid_or_conflicting_rollover_without_mutation() {
    let dir = TempDir::new().expect("temp dir");
    init_active_sprint_with_unfinished_items(dir.path());
    let items_before = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let sprints_before = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));

    pinto(dir.path())
        .args(["sprint", "close", "S-1", "--rollover", "S-404"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("S-404"));
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        items_before
    );
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"])),
        sprints_before
    );

    pinto(dir.path())
        .args(["sprint", "close", "S-1", "--rollover", "S-2", "--release"])
        .assert()
        .failure();
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        items_before
    );
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"])),
        sprints_before
    );
}

#[cfg(feature = "sqlite")]
#[test]
fn sprint_close_rollover_persists_across_the_sqlite_backend() {
    let dir = TempDir::new().expect("temp dir");
    init_active_sprint_with_unfinished_items(dir.path());
    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "close", "S-1", "--rollover", "S-2"])
        .assert()
        .success();

    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-2", "--json"]))["sprint"],
        "S-2"
    );
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-3", "--json"]))["sprint"],
        "S-2"
    );
    let sprints = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    assert_eq!(sprints[0]["state"], "closed");
    assert_eq!(sprints[0]["spillover_points"], 5);
    assert_eq!(sprints[0]["spillover_items"], 2);
    assert_eq!(sprints[0]["unestimated_spillover_items"], 1);
}

#[test]
fn sprint_close_from_planned_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid sprint transition"));
}

#[test]
fn sprint_add_and_unassign_assigns_and_unassigns_pbi() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // The assignment is reflected by show.
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Assigned T-1 to sprint S-1"));
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("S-1"));

    // Unassignment removes the sprint field.
    pinto(dir.path())
        .args(["sprint", "unassign", "S-1", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Unassigned T-1 from sprint S-1"));
    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-1.md")).unwrap();
    assert!(!task.contains("sprint ="), "sprint field cleared: {task}");
}

#[test]
fn sprint_add_by_status_assigns_ranked_items_up_to_limit() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    for title in ["First", "Second", "Third"] {
        pinto(dir.path()).args(["add", title]).assert().success();
    }
    pinto(dir.path())
        .args(["move", "T-3", "in-progress"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "add", "S-1", "--status", "todo", "--limit", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Assigned T-1 to sprint S-1"))
        .stdout(predicate::str::contains("Assigned T-2 to sprint S-1"))
        .stdout(predicate::str::contains("Assigned T-3 to sprint S-1").not());

    let assigned = json_stdout(pinto(dir.path()).args(["list", "--sprint", "S-1", "--json"]));
    let ids: Vec<_> = assigned
        .as_array()
        .expect("list JSON array")
        .iter()
        .map(|item| item["id"].as_str().expect("item id"))
        .collect();
    assert_eq!(ids, ["T-1", "T-2"]);
}

#[test]
fn sprint_add_by_status_without_limit_assigns_all_matching_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    pinto(dir.path())
        .args(["sprint", "add", "S-1", "--status", "todo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Assigned T-1 to sprint S-1"))
        .stdout(predicate::str::contains("Assigned T-2 to sprint S-1"));

    let assigned = json_stdout(pinto(dir.path()).args(["list", "--sprint", "S-1", "--json"]));
    assert_eq!(assigned.as_array().expect("list JSON array").len(), 2);
}

#[test]
fn sprint_add_by_status_rejects_invalid_input_without_changes() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["sprint", "add", "S-1", "--status", "missing"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown status"));
    assert!(show_json(pinto(dir.path()).args(["show", "T-1", "--json"]))["sprint"].is_null());

    pinto(dir.path())
        .args(["sprint", "add", "S-1", "--status", "todo", "--limit", "0"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("must be at least 1"));
    assert!(show_json(pinto(dir.path()).args(["show", "T-1", "--json"]))["sprint"].is_null());
}

#[test]
fn sprint_add_by_status_rejects_closed_sprint_without_changes() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One", "--goal", "Ship it"])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();
    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "add", "S-1", "--status", "todo"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("closed sprint"));
    assert!(show_json(pinto(dir.path()).args(["show", "T-1", "--json"]))["sprint"].is_null());
}

#[test]
fn sprint_delete_unassigns_pbis_and_keeps_their_data() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Planning"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Keep this PBI"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "remove", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted sprint S-1"));

    assert!(
        json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]))
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["title"], "Keep this PBI");
    assert!(item["sprint"].is_null(), "deleted sprint is unassigned");
}

#[test]
fn sprint_rm_alias_removes_a_sprint() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Planning"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "rm", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted sprint S-1"));
}

#[test]
fn sprint_add_to_missing_sprint_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["sprint", "add", "S-9", "T-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("sprint not found"));
}

#[test]
fn sprint_add_to_malformed_sprint_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["sprint", "add", "S 1", "T-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid sprint id"));

    let item = json_stdout(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["sprint"], serde_json::Value::Null);
}

#[test]
fn sprint_add_rejects_closed_sprint_as_user_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--goal",
            "Ship the sprint",
        ])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();
    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("closed sprint"))
        .stderr(predicate::str::contains("planned or active"));

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["sprint"], serde_json::Value::Null);
}

#[test]
fn sprint_add_allows_active_and_unassign_allows_closed_sprint() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--goal",
            "Ship the sprint",
        ])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Assigned T-1 to sprint S-1"));
    pinto(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "unassign", "S-1", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Unassigned T-1 from sprint S-1"));
    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["sprint"], serde_json::Value::Null);
}

#[test]
fn sprint_list_shows_sprints_in_creation_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "First"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-2",
            "Second",
            "--goal",
            "Ship the sprint",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "start", "S-2"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("S-1"))
        .stdout(predicate::str::contains("planned"))
        .stdout(predicate::str::contains("S-2"))
        .stdout(predicate::str::contains("active"));
}

#[test]
fn sprint_list_empty_reports_no_sprints() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["sprint", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No sprints"));
}

#[test]
fn sprint_velocity_lists_completed_points_average_and_unestimated_work() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    for id in ["S-1", "S-2"] {
        pinto(dir.path())
            .args(["sprint", "new", id, id])
            .assert()
            .success();
    }
    pinto(dir.path())
        .args(["add", "Estimated", "--points", "3"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Unestimated"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Incomplete", "--points", "8"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-2", "T-2"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-2", "T-3"])
        .assert()
        .success();
    for id in ["T-1", "T-2"] {
        pinto(dir.path())
            .args(["move", id, "done"])
            .assert()
            .success();
    }

    pinto(dir.path())
        .args(["sprint", "velocity", "--recent", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Velocity (last 2 sprints)"))
        .stdout(predicate::str::contains("S-1"))
        .stdout(predicate::str::contains("S-2"))
        .stdout(predicate::str::contains("Average: 1.5 points"))
        .stdout(predicate::str::contains("Change: -100.0% vs prior average"))
        .stdout(predicate::str::contains("unestimated: 1"))
        .stdout(predicate::str::contains("incomplete: 1"));
}

#[test]
fn sprint_velocity_handles_an_empty_board() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("sprint")
        .arg("velocity")
        .assert()
        .success()
        .stdout(predicate::str::contains("No sprints"));
}

#[test]
fn sprint_burndown_renders_chart_with_period_and_remaining() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-08",
        ])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-2"])
        .assert()
        .success();
    // Move T-1 to done; the remaining work decreases.
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "burndown", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("burndown (items)"))
        .stdout(predicate::str::contains("Period 2026-07-06 → 2026-07-08"));
}

#[test]
fn sprint_burndown_json_emits_daily_series() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-08",
        ])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "burndown", "S-1", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"metric\": \"items\""))
        .stdout(predicate::str::contains("\"remaining\""))
        .stdout(predicate::str::contains("\"date\": \"2026-07-06\""));
}

#[test]
fn sprint_burndown_without_period_guides_user_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();

    // Without planned dates, the report cannot be rendered and explains how to configure them.
    pinto(dir.path())
        .args(["sprint", "burndown", "S-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no start/end dates"));
}

#[test]
fn sprint_burndown_without_items_guides_user_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Sprint One",
            "--start",
            "2026-07-06",
            "--end",
            "2026-07-08",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "burndown", "S-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no assigned items"));
}
