---
name: pinto-add-pbi
description: Add one or more Product Backlog Items to a pinto board with non-interactive pinto automate plans. Use when a user asks to capture work in the Product Backlog, especially when the request contains a bullet list and requires the default item template, stacked priority ordering, or English PBI text regardless of the prompt language.
---

# Pinto Add PBI

Create well-formed Product Backlog Items in pinto from the user's work requests without opening an editor.

## Extract and translate the requests

1. Treat each top-level bullet as exactly one PBI, in the order given. Treat nested bullets as details or candidate acceptance criteria, not additional PBIs, unless the user explicitly asks to split them.
2. If the request is not a list, create one PBI. Do not create extra PBIs for explanatory prose.
3. Write the saved PBI title, Summary, and Acceptance Criteria in English regardless of the prompt language. Preserve proper nouns, product names, code identifiers, commands, flags, URLs, and other exact tokens.
4. Make the title concise and outcome-oriented. Write a short Summary describing the requested result. Turn the request's observable outcomes into concise, testable checkbox criteria. Do not invent implementation details or unrelated scope; when the request is underspecified, use the smallest reasonable interpretation.

## Use two non-interactive automation plans

Use the existing board from its project root. Do not run `pinto init` or change board configuration unless the user explicitly asks for it.

Use JSON plan files with `pinto automate`; each command must be an argv array. Do not use shell loops, command substitution, `$EDITOR`, or an editor shim. Do not combine `--template default` and `--body` in an `add` command: pinto appends the body after the template, leaving duplicate headings or placeholders.

Before building a plan, verify that `.pinto/templates/item/default.md` exists and is readable. Stop with an actionable error if it is missing; do not silently create or replace the project's template.

### 1. Allocate the PBIs with the default template

Build a temporary add plan in the user's bullet order. Use `--template default` and pass through points, labels, sprint, parent, or dependencies only when the user explicitly provides them or the values are unambiguous.

```json
{
  "commands": [
    ["add", "<English title 1>", "--template", "default"],
    ["add", "<English title 2>", "--template", "default"]
  ]
}
```

Validate the plan before applying it, then apply it and retain the JSON report:

```text
pinto automate --plan <add-plan.json> --dry-run --json
pinto automate --plan <add-plan.json> --json
```

Read each successful command's `created_ids` from the apply report. Keep the IDs paired with the original bullet order. Never assume the next ID or extract it from a human-readable message. If the add plan partially fails, stop, report successful/failed/skipped commands, and do not rerun the entire plan or create duplicate PBIs.

### 2. Fill the bodies and apply stack ordering

Build a second plan using the captured IDs. Use `edit --body` to replace the placeholder body created by the default template, then immediately reorder that same item to the top. Generate one edit/reorder pair per bullet in the original input order.

```json
{
  "commands": [
    ["edit", "<created-id-1>", "--body", "# **Summary**\n\n<English summary>\n\n# **Acceptance Criteria**\n\n- [ ] <observable criterion>"],
    ["reorder", "<created-id-1>", "--top"],
    ["edit", "<created-id-2>", "--body", "# **Summary**\n\n<English summary>\n\n# **Acceptance Criteria**\n\n- [ ] <observable criterion>"],
    ["reorder", "<created-id-2>", "--top"]
  ]
}
```

Replace the template comments and placeholder checkbox. Include every criterion as a `- [ ]` checkbox. Store multiline Markdown as escaped newlines in the JSON plan; use a plan file rather than relying on shell quoting.

Validate and apply the second plan:

```text
pinto automate --plan <update-plan.json> --dry-run --json
pinto automate --plan <update-plan.json> --json
```

The edit/reorder pairs must remain in input order. For bullets `A`, `B`, `C`, the final top-to-bottom order is `C`, `B`, `A`: each later `--top` operation pushes its item above the earlier ones. `--top` is scoped to the item's sibling group, so this guarantee applies independently within each shared status/parent group.

If the second plan fails, use its JSON report to identify completed edit/reorder commands and create a recovery plan only for the remaining operations. Do not rerun successful `add` commands. If an edit succeeded but its reorder failed, rerun only that reorder.

Prefer `pinto automate ...` when using an installed binary. Use `cargo run --quiet -- automate ...` from the pinto source checkout when following the repository dogfooding workflow.

## Verify the result

After the update plan succeeds:

1. List the affected workflow column in rank order and confirm the new IDs appear in reverse input order. Use JSON output when it makes the check reliable:

   ```text
   pinto list --status todo --json
   ```

   Replace `todo` with the board's first workflow column when it differs.
2. Show each created item and confirm its title, English Summary, English Acceptance Criteria, and absence of template placeholders:

   ```text
   pinto show <ID> --plain
   ```

3. Summarize the created IDs and their final order. Do not create a separate Git commit for the PBIs unless the user explicitly requests one; pinto's configured repository backend handles its own write history.
