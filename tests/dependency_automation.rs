//! Guards for automated dependency and GitHub Actions maintenance (P-52).
//!
//! These integration tests lock the maintenance contract in place: every
//! Actions reference stays pinned to a full commit SHA with a human-readable
//! version comment, scheduled automation proposes Cargo and Actions updates,
//! the dependency policy denies unrecorded duplicate versions, and the
//! maintainer guidance and demo stay present.

use std::fs;
use std::path::Path;

use regex::Regex;

fn repository_file(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path))
        .map(|contents| contents.replace("\r\n", "\n"))
        .unwrap_or_else(|error| panic!("expected repository file {path}: {error}"))
}

const WORKFLOWS: &[&str] = &[".github/workflows/ci.yml", ".github/workflows/pages.yml"];

/// AC1: every third-party Actions reference is pinned to a 40-character commit
/// SHA and keeps a trailing human-readable version comment.
#[test]
fn every_action_reference_is_sha_pinned_with_a_version_comment() {
    // `owner/repo@<40 hex> # <version>` — the comment is what Dependabot and
    // humans read to know which release the SHA corresponds to.
    let pinned = Regex::new(r"^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+@[0-9a-f]{40} # \S+").unwrap();

    let mut references = 0;
    for workflow in WORKFLOWS {
        let contents = repository_file(workflow);
        for line in contents.lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("uses:") else {
                continue;
            };
            let reference = rest.trim();
            // Local composite actions (`./.github/...`) never need pinning.
            if reference.starts_with("./") {
                continue;
            }
            references += 1;
            assert!(
                pinned.is_match(reference),
                "{workflow} has an Actions reference that is not SHA-pinned with a version comment: {reference}"
            );
        }
    }
    assert!(
        references >= 5,
        "expected the workflows to reference several actions, found {references}"
    );
}

/// AC2: scheduled automation proposes updates for both the Cargo and the
/// GitHub Actions ecosystems.
#[test]
fn dependabot_proposes_cargo_and_actions_updates_on_a_schedule() {
    let config = repository_file(".github/dependabot.yml");
    let docs = yaml_rust2::YamlLoader::load_from_str(&config)
        .unwrap_or_else(|error| panic!(".github/dependabot.yml is not valid YAML: {error}"));
    let root = docs.first().expect("dependabot.yml has a document");

    assert_eq!(
        root["version"].as_i64(),
        Some(2),
        "dependabot.yml must declare schema version 2"
    );

    let updates = root["updates"]
        .as_vec()
        .expect("dependabot.yml declares an updates list");
    let mut ecosystems = Vec::new();
    for entry in updates {
        let ecosystem = entry["package-ecosystem"]
            .as_str()
            .expect("each update entry names a package-ecosystem")
            .to_string();
        assert!(
            entry["schedule"]["interval"].as_str().is_some(),
            "the {ecosystem} update entry must declare a schedule interval"
        );
        ecosystems.push(ecosystem);
    }
    for required in ["cargo", "github-actions"] {
        assert!(
            ecosystems.iter().any(|e| e == required),
            "dependabot.yml must propose {required} updates; found {ecosystems:?}"
        );
    }
}

/// AC4: the dependency policy denies duplicate versions and records the
/// currently approved duplicates so new ones fail the gate.
#[test]
fn dependency_policy_denies_and_records_duplicate_versions() {
    let deny = repository_file("deny.toml");
    let document = toml::from_str::<toml::Value>(&deny).expect("deny.toml is valid TOML");
    let bans = document
        .get("bans")
        .and_then(toml::Value::as_table)
        .expect("deny.toml declares a [bans] section");

    assert_eq!(
        bans.get("multiple-versions").and_then(toml::Value::as_str),
        Some("deny"),
        "deny.toml must deny multiple versions so new duplicates fail the gate"
    );

    let skip = bans
        .get("skip")
        .and_then(toml::Value::as_array)
        .expect("deny.toml records approved duplicate versions under [bans].skip");
    let recorded: Vec<String> = skip
        .iter()
        .filter_map(|entry| entry.get("crate").and_then(toml::Value::as_str))
        .map(str::to_string)
        .collect();
    for approved in ["hashbrown", "unicode-width"] {
        assert!(
            recorded.iter().any(|c| c.starts_with(approved)),
            "deny.toml must record the approved duplicate {approved}; found {recorded:?}"
        );
    }
}

/// AC3 + AC5: the maintainer docs explain the maintained YAML decision and how
/// to review and validate automated update proposals.
#[test]
fn maintainer_docs_explain_reviewing_automated_proposals() {
    let docs = repository_file("docs/dependencies.md");
    for marker in [
        "## Automated dependency and Actions maintenance",
        "dependabot.yml",
        "commit SHA",
        "mise run check",
        "mise run audit",
        "mise run deny",
        "yaml-rust2",
        "duplicate",
    ] {
        assert!(
            docs.contains(marker),
            "docs/dependencies.md omits maintenance guidance: {marker}"
        );
    }
}
