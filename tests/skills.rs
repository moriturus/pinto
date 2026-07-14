//! Integration checks for the distributable pinto agent skill.

use std::fs;
use std::path::Path;

use assert_cmd::Command;

const SKILL_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/pinto-workflow/SKILL.md"
);
const GUIDE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/skills.md");
const DEMO_README_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/demos/single/skills/README.md");

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn skill_body(skill: &str) -> &str {
    let mut sections = skill.splitn(3, "---");
    assert_eq!(
        sections.next(),
        Some(""),
        "SKILL.md must start with YAML frontmatter"
    );
    sections.next().expect("SKILL.md must contain frontmatter");
    sections
        .next()
        .expect("SKILL.md must contain a Markdown body")
}

#[test]
fn skill_has_agent_skills_metadata_and_guidance() {
    let skill = read(SKILL_PATH);
    let mut sections = skill.splitn(3, "---");
    assert_eq!(
        sections.next(),
        Some(""),
        "SKILL.md must start with YAML frontmatter"
    );
    let frontmatter = sections.next().expect("SKILL.md must contain frontmatter");
    let description = frontmatter
        .lines()
        .find_map(|line| line.strip_prefix("description:"))
        .map(str::trim)
        .unwrap_or_default();

    assert!(
        frontmatter
            .lines()
            .any(|line| line.trim() == "name: pinto-workflow"),
        "skill name must match its directory"
    );
    assert!(
        !description.is_empty(),
        "skill description must not be empty"
    );

    let body = skill_body(&skill).to_ascii_lowercase();
    for phrase in [
        "lightweight, local-first scrum backlog and kanban board",
        ".pinto/",
        "pinto init",
        "pinto automate --plan",
        "outside a pinto checkout",
        "use the installed pinto binary",
        "pinto list --status todo",
        "dogfooding",
        "npx skills add",
        "npx skills use",
    ] {
        assert!(body.contains(phrase), "skill is missing guidance: {phrase}");
    }
    assert!(
        !body.contains("cargo run"),
        "distributable skill must use pinto commands"
    );
    for obsolete_name in ["sboard", "pinto-core"] {
        assert!(
            !body.contains(obsolete_name),
            "skill must not mention obsolete project name {obsolete_name}"
        );
    }
}

#[test]
fn skill_commands_exist_in_the_current_cli() {
    let skill = read(SKILL_PATH);
    for line in skill.lines() {
        let command_line = line.strip_prefix("pinto ");
        let Some(command_line) = command_line else {
            continue;
        };
        let command = command_line
            .split_whitespace()
            .next()
            .expect("documented command has a name");
        Command::cargo_bin("pinto")
            .expect("binary builds")
            .args([command, "--help"])
            .assert()
            .success()
            .stdout(predicates::str::contains("Usage:"));
    }
}

#[test]
fn installation_guide_and_demo_are_reproducible() {
    let guide = read(GUIDE_PATH).to_ascii_lowercase();
    for phrase in [
        "npx skills add . --list",
        "npx skills add . --skill pinto-workflow",
        "npx skills use . --skill pinto-workflow",
        "--agent codex",
        "--copy",
        "other repositories",
        "pinto list --status todo",
        "pinto automate --plan",
    ] {
        assert!(
            guide.contains(phrase),
            "guide is missing installation step: {phrase}"
        );
    }
    assert!(
        !guide.contains("cargo run"),
        "installation guide must use pinto commands"
    );

    let demo_readme = read(DEMO_README_PATH).to_ascii_lowercase();
    assert!(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/demos/single/skills/.pinto"
        ))
        .is_dir(),
        "skills demo must contain a pinto data directory"
    );
    Command::cargo_bin("pinto")
        .expect("binary builds")
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/demos/single/skills"))
        .args(["list", "--long"])
        .assert()
        .success()
        .stdout(predicates::str::contains("T-1"))
        .stdout(predicates::str::contains("T-2"));
    for phrase in ["pinto list --long", "skills use", ".pinto/"] {
        assert!(
            demo_readme.contains(phrase),
            "skills demo README is missing: {phrase}"
        );
    }
}
