---
name: pinto-add-pbi
description: Add one or more Product Backlog Items to a pinto board with non-interactive pinto automate plans. Use when a user asks to capture work in the Product Backlog, especially when the request contains a bullet list and requires the default item template, LILO priority ordering, or English PBI text regardless of the prompt language.
---

# Pinto Add PBI

Create well-formed Product Backlog Items in pinto from the user's work requests without opening an editor.

## Coordinate with the board workflow

Follow `@pinto-workflow` for board discovery, write authority, and verification.
Use it for the shared automate plan and recovery rules; this skill specializes
in translating requests into PBIs and applying the one-plan capture flow.
Treat IDs from a real apply report as authoritative and pair them with the
original bullet order. Never use dry-run IDs as authoritative item IDs.

## Extract and translate the requests

1. Treat each top-level bullet as exactly one PBI, in the order given. Treat nested bullets as details or candidate acceptance criteria, not additional PBIs, unless the user explicitly asks to split them.
2. If the request is not a list, create one PBI. Do not create extra PBIs for explanatory prose.
3. Write the saved PBI title, Summary, and Acceptance Criteria in English regardless of the prompt language. Preserve proper nouns, product names, code identifiers, commands, flags, URLs, and other exact tokens.
4. Make the title concise and outcome-oriented. Write a short Summary describing the requested result. Turn the request's observable outcomes into concise, testable checkbox criteria. Do not invent implementation details or unrelated scope; when the request is underspecified, use the smallest reasonable interpretation.

## Use one non-interactive automation plan

Use the existing board from its project root. Do not run `pinto init` or change board configuration unless the user explicitly asks for it.

Use one JSON plan file with `pinto automate`; each command must be an argv array. Do not use shell loops, command substitution, `$EDITOR`, or an editor shim. Do not combine `--template default` and `--body` in an `add` command: pinto appends the body after the template, leaving duplicate headings or placeholders. Add each PBI with the default template, then use a later `edit --body` command in the same plan to replace that template body.

Before building a plan, verify that `.pinto/templates/item/default.md` exists and is readable. Stop with an actionable error if it is missing; do not silently create or replace the project's template.

### Build the plan in input order

Build one plan in the user's bullet order. Use `--template default` for each `add`, and pass through points, labels, sprint, parent, or dependencies only when the user explicitly provides them or the values are unambiguous. Follow each `add` with an `edit --body` for the same item, then append `reorder --top` commands in reverse input order. This gives LILO (last in, last out) priority: bullets `A`, `B`, `C` finish top-to-bottom as `A`, `B`, `C` while the whole group is promoted ahead of existing siblings. Reference each earlier add's actual ID with a complete zero-based placeholder.

```json
{
  "commands": [
    ["add", "<English title 1>", "--template", "default"],
    ["edit", "@command[0].created_ids[0]", "--body", "# **Summary**\n\n<English summary>\n\n# **Acceptance Criteria**\n\n- [ ] <observable criterion>"],
    ["add", "<English title 2>", "--template", "default"],
    ["edit", "@command[2].created_ids[0]", "--body", "# **Summary**\n\n<English summary>\n\n# **Acceptance Criteria**\n\n- [ ] <observable criterion>"],
    ["reorder", "@command[2].created_ids[0]", "--top"],
    ["reorder", "@command[0].created_ids[0]", "--top"]
  ]
}
```

Replace the template comments and placeholder checkbox. Include every criterion as a `- [ ]` checkbox. Store multiline Markdown as escaped newlines in the JSON plan; use a plan file rather than relying on shell quoting. Validate the complete plan before applying it, then apply the same plan and retain the JSON report:

```text
pinto automate --plan <plan.json> --dry-run --json
pinto automate --plan <plan.json> --json
```

Read successful commands' `created_ids` from the apply report and keep them paired with the original bullet order. Never assume the next ID or extract it from a human-readable message. The add/edit pairs must remain in input order, while the final `reorder --top` commands must be in reverse input order. For bullets `A`, `B`, `C`, the final top-to-bottom order is `A`, `B`, `C`: the last-in item is moved first and the first-in item is moved last. `--top` is scoped to the item's sibling group, so this guarantee applies independently within each shared status/parent group.

If the plan partially fails, use its JSON report to identify the successful prefix and the first failed command. Do not rerun successful `add` commands or the entire plan. Use authoritative IDs from the apply report in a recovery plan for only the remaining `edit`/`reorder` operations; if an edit succeeded but its reorder failed, rerun only that reorder. Never use IDs from the dry-run report.

Prefer `pinto automate ...` when using an installed binary. Use `cargo run --quiet -- automate ...` from the pinto source checkout when following the repository dogfooding workflow.

## Verify the result

After the plan succeeds:

1. List the affected workflow column in rank order and confirm the new IDs appear in input order under the LILO rule. Use JSON output when it makes the check reliable:

   ```text
   pinto list --status todo --json
   ```

   Replace `todo` with the board's first workflow column when it differs.
2. Show each created item and confirm its title, English Summary, English Acceptance Criteria, and absence of template placeholders:

   ```text
   pinto show <ID> --plain
   ```

3. Summarize the created IDs and their final order. Do not create a separate Git commit for the PBIs unless the user explicitly requests one; pinto's configured repository backend handles its own write history.
