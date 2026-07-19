<!--
Thank you for opening a pull request.
Review CONTRIBUTING.md before merging and complete the checklist below.
-->

## Summary

<!-- Briefly describe what changed and why. Include a related issue or planning reference when applicable. -->

## Changes

<!-- List the main changes. -->

-

## Checklist

- [ ] **TDD**: Write a failing test first (Red → Green → Refactor) and protect the behavior with tests.
- [ ] `mise run check` (tests, zero Clippy warnings, Rust and mdBook docs, and formatting) passes locally.
- [ ] Public APIs and non-trivial logic have documentation comments.
- [ ] User-facing help and errors are concise and explain how to recover.
- [ ] Affected documentation is updated.
- [ ] If a dependency was added, its necessity is explained below to preserve the lightweight design.
- [ ] **Commit boundaries:** Changes are in small, green commits; cross-cutting work separates data, service, CLI, and documentation changes where practical.
- [ ] **Acceptance review:** Acceptance conditions were reviewed before a large change, and the relevant scope or migration decisions are recorded.
- [ ] **Maintainer verification:** A destructive or release-related change has documented risk, verification, and recovery steps.

## Design alignment

<!-- Explain whether the change is needed for Scrum work and preserves the lightweight design. -->

- **Scrum-related need:**
- **Why existing functionality is insufficient:**
- **Dependency impact:** None, or explain each added/changed dependency and why it is necessary.
- **Persistence impact:** None, or describe changes to board files, schemas, or stored data.
- **Migration and compatibility impact:** None, or describe migration, downgrade, and recovery guidance.
- **Maintainer verification:** Record the final decision, applicable checks, and any follow-up actions.

For a new command, backend, or report, keep this record even when no dependency
changes are required.
