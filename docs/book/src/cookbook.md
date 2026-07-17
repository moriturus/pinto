# Cookbook

This chapter collects goal-oriented recipes for everyday pinto work. Every
recipe states its prerequisites, the exact command, and how to verify the
result. All of them run in a clean temporary directory, so you can replay the
whole chapter without touching an existing board:

```bash
mkdir -p /tmp/pinto-cookbook && cd /tmp/pinto-cookbook
pinto init
```

The recipes call the installed `pinto` binary. Inside the repository you can
substitute `cargo run --` for `pinto`, as described in
[Dogfooding](dogfooding.md). A ready-made board for the pipeline recipes lives
in [`demos/single/cookbook`](https://github.com/moriturus/pinto/tree/main/demos/single/cookbook).

## Backlog basics

### Seed a small backlog

**Prerequisites:** an initialized board (`pinto init`).

```bash
pinto add "Design the login form" --points 3 --label ui auth
pinto add "Implement the login API" --points 5 --label api auth
pinto add "Write onboarding docs" --points 2 --label docs
pinto add "Fix the session timeout bug" --points 1 --label bug --label auth
pinto add "Refactor the storage layer" --points 8 --label refactor
```

One `--label` accepts all following label values until the next option. The
repeated form used by the session-timeout item is equivalent and remains
supported.

**Verify:** each command prints the assigned ID (`Created T-1 …` through
`T-5`). The remaining recipes assume these five PBIs.

### List and inspect

**Prerequisites:** the seeded backlog above.

```bash
pinto list --status todo
pinto show T-1
pinto list --long
pinto list --json
```

**Verify:** `pinto list` prints one line per PBI — ID, status, title, points in
parentheses, labels in brackets. `--long` adds dates and other columns; its
noninteractive output has no header row. `--json` emits machine-readable
output for scripts.

### Move work through the workflow

**Prerequisites:** the seeded backlog above.

```bash
pinto move T-1 in-progress
pinto move T-2 review
pinto board
```

**Verify:** `pinto board` shows `T-1` under `in-progress` and `T-2` under
`review`. The last operand to `pinto move` is the destination column, exactly
like Unix `mv`.

## Sprint recipes

### Create a sprint and assign work in bulk

**Prerequisites:** the seeded backlog with `T-3`, `T-4`, and `T-5` still in
`todo`.

```bash
pinto sprint new S-1 "Sprint 1" --goal "Ship the login flow" \
  --start 2026-07-13 --end 2026-07-27
pinto sprint add S-1 --status todo --limit 2
pinto sprint start S-1
```

**Verify:** the bulk assignment picks the two highest-ranked `todo` PBIs
(`Assigned T-3 to sprint S-1`, `Assigned T-4 to sprint S-1`). Omit `--limit`
to assign every match, or pass a single ID (`pinto sprint add S-1 T-5`)
instead of `--status`. `pinto sprint list` now reports `S-1` as `active`.

The close-out recipe completes one Sprint PBI, rolls the unfinished PBI into the next Sprint while
closing, then runs the reports; see
[Close out and report](#close-out-and-report).

## Unix text-stream recipes

pinto prints plain text on purpose, so the standard Unix toolbox composes with
it. Two properties make the default `pinto list` output easy to process:

- columns are separated by runs of spaces, so `tr -s ' '` normalizes a line to
  single-space-separated fields;
- the ID is always the first field and the status the second, while the title
  may contain spaces.

All recipes below stick to POSIX options and behave the same with the GNU
and BSD userlands, including the BSD tools shipped with macOS. Portability notes are
called out per recipe — for example, in-place editing differs between GNU
`sed -i` and BSD `sed -i ''`, so the recipes always write to standard output
instead. They assume the board built in the previous sections.

### 1. Extract IDs with cut

**Prerequisites:** the seeded backlog.

```bash
pinto list --status todo | cut -d' ' -f1
```

**Verify:** only the ID column remains:

```text
T-3
T-4
T-5
```

The ID never contains a space, so cutting the first space-delimited field is
safe even though later columns are padded with multiple spaces.

### 2. Filter by label with grep

**Prerequisites:** the seeded backlog.

```bash
pinto list | grep -E '\[[^]]*auth'
```

**Verify:** only the three PBIs labeled `auth` are printed. The pattern
anchors on the label list in brackets, so a title that merely mentions
"auth" does not match. `grep -E` (extended regular expressions) is POSIX and
works with both GNU and BSD grep.

### 3. Count PBIs per status with sort and uniq

**Prerequisites:** the seeded backlog.

```bash
pinto list | tr -s ' ' | cut -d' ' -f2 | sort | uniq -c
```

**Verify:** a frequency table of the status column:

```text
   1 in-progress
   1 review
   3 todo
```

`uniq` only merges adjacent lines, so the `sort` before it is required.

### 4. Count matches with wc

**Prerequisites:** the seeded backlog.

```bash
pinto list --status todo | wc -l
```

**Verify:** prints `3`. BSD `wc` pads the number with leading spaces; pipe
through `tr -d ' '` if a script needs the bare digits.

### 5. Take the top of the backlog with head

**Prerequisites:** the seeded backlog.

```bash
pinto list --status todo | head -n 2
```

**Verify:** the two highest-ranked `todo` PBIs, in backlog rank order — the
same two that `pinto sprint add S-1 --status todo --limit 2` would assign.

### 6. Take the last records with tail

**Prerequisites:** the seeded backlog.

```bash
pinto list --long | tail -n 2
```

**Verify:** only the last two data rows (`T-4` and `T-5` with the seed data)
remain. Because pinto's noninteractive `--long` output is data-only, `tail -n
2` selects records rather than skipping a header. When an upstream command
does emit a header, `tail -n +2` ("start at line 2") is POSIX and portable,
unlike the historical `tail +2` form.

### 7. Normalize aligned columns with tr

**Prerequisites:** the seeded backlog.

```bash
pinto list | tr -s ' '
```

**Verify:** every run of spaces collapses to a single space:

```text
T-1 in-progress Design the login form (3) [ui, auth]
```

This is the standard first step before `cut`, `join`, or any tool that
expects a single-character field delimiter.

### 8. Render a Markdown checklist with sed

**Prerequisites:** the seeded backlog.

```bash
pinto list --status todo | sed -E 's/^(T-[0-9]+)[[:space:]]+[[:alnum:]-]+[[:space:]]+/- [ ] \1 /'
```

**Verify:** a paste-ready checklist for a standup note:

```text
- [ ] T-3 Write onboarding docs  (2)  [docs]
- [ ] T-4 Fix the session timeout bug  (1)  [bug, auth]
- [ ] T-5 Refactor the storage layer  (8)  [refactor]
```

`sed -E` is supported by both GNU and BSD sed. Avoid `sed -i` in shared
scripts: GNU accepts `sed -i`, BSD requires `sed -i ''`.

### 9. Collect IDs onto one line with paste

**Prerequisites:** the seeded backlog.

```bash
pinto list --status todo | cut -d' ' -f1 | paste -sd' ' -
```

**Verify:** prints `T-3 T-4 T-5` on a single line, ready to splice into
another command. The trailing `-` operand is required by BSD paste to read
standard input; GNU paste accepts it too, so always write it.

### 10. Join sprint assignments with statuses using join

**Prerequisites:** the sprint recipes above (`T-3` and `T-4` assigned to
`S-1`).

```bash
pinto list | tr -s ' ' | cut -d' ' -f1,2 | sort > status.txt
pinto list --sprint S-1 | cut -d' ' -f1 | sort | join - status.txt
```

**Verify:** each sprint item paired with its current board status:

```text
T-3 todo
T-4 todo
```

`join` needs both inputs sorted on the join field; the `-` reads the sprint
IDs from standard input while `status.txt` supplies the second column.

### 11. Feed a pipeline back into pinto

**Prerequisites:** the seeded backlog, with `T-3` and `T-4` still in `todo`.

```bash
pinto move $(pinto list --status todo | head -n 2 | cut -d' ' -f1) in-progress
```

**Verify:** pinto confirms each transition (`Moved T-3 to in-progress`,
`Moved T-4 to in-progress`), and `pinto list --status in-progress` shows the
moved PBIs. This composes recipes 1 and 5: the pipeline selects the top of the
backlog and the command substitution feeds the IDs back into `pinto move`.

### 12. Sum story points with paste and bc

**Prerequisites:** every listed PBI has story points; `bc` (POSIX) is
installed.

```bash
pinto list --sprint S-1 | tr -s ' ' | sed -E 's/.*\(([0-9]+)\).*/\1/' | paste -sd+ - | bc
```

**Verify:** prints the total committed points for `S-1` (`3` with the seed
data: 2 + 1). `sed` isolates the points, `paste -sd+ -` folds them into an
arithmetic expression (`2+1`), and `bc` evaluates it.

## Close out and report

**Prerequisites:** the sprint recipes above; recipe 11 already moved `T-3`
and `T-4` to `in-progress`.

```bash
pinto sprint new S-2 "Next Sprint"
pinto move T-3 done
pinto sprint close S-1 --rollover S-2
pinto sprint velocity
pinto sprint burndown S-1
pinto cycletime --sprint S-1
```

**Verify:** `pinto sprint velocity` reports 2 completed points for `S-1` and separately reports
1 spillover point in 1 item. `T-4` is now assigned to `S-2`; its point is not included in the
velocity average or change. `burndown` draws a chart over the planned period, and `cycletime`
lists lead and cycle times for completed PBIs. Use `--release` instead of `--rollover S-2` when
unfinished work should return to the unassigned backlog.
