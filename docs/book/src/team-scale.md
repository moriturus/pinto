# Team-scale best practices

pinto can support different working styles without making every user adopt the
same amount of process. Choose the smallest set of features that gives the
people doing the work enough shared structure. The recommendations below are
guidelines, not hard limits: a team can adopt more structure as its coordination
needs grow.

## Individual development

For one developer or a personal project, use pinto without Scrum features. Keep
an ordered Product Backlog and move PBIs through the Kanban workflow with the
everyday commands:

```bash
pinto add "Write the release notes"
pinto list
pinto show T-1
pinto move T-1 in-progress
```

Skip Sprints, Sprint goals, and capacity planning when there is no team that
needs those coordination tools. This keeps pinto's local-first workflow useful
for personal planning without adding ceremonies or bookkeeping that do not
improve the work.

## Small teams

For a small team working toward a shared product goal, use pinto with Scrum features.
Keep the Product Backlog as the team's ordered source of work, then
use Sprints, Sprint goals, points, and the Kanban workflow to make the plan and
current progress visible:

```bash
pinto sprint new S-1 "Sprint 1" --goal "Ship the first release"
pinto sprint add S-1 T-1
pinto sprint start S-1
```

This amount of structure gives a small team a shared planning and review rhythm
while preserving pinto's lightweight, local-first model. Configure only the
workflow and Sprint practices the team actually uses.

## Larger teams

For a larger team or a board changed by many contributors, use the Git backend
in a dedicated repository for the shared pinto board:

```toml
[storage]
backend = "git"
```

Keep the board's `.pinto/` directory in that dedicated repository rather than
mixing board commits with application source changes. The Git backend keeps
pinto's file-based board and CLI while creating one Git commit for each complete
write operation. The dedicated repository gives a larger team a durable review,
permission, and recovery boundary for Product Backlog, Sprint, and workflow
changes. Use the [merging shared boards](merging.md) guide when multiple clones
need to combine board changes. The Git backend does not remove Scrum features;
it adds the collaboration boundary that a larger team needs.
