# Introduction

pinto is a lightweight, local-first Scrum backlog and Kanban board for the
terminal. It keeps Product Backlog Items (PBIs), Sprints, and workflow state in
plain text so that the board remains easy to inspect, version, and recover with
Git.

The project deliberately has a small vocabulary:

- **Product Backlog** — the ordered list of work.
- **Sprint** — a time-boxed selection of PBIs with a goal.
- **Kanban workflow** — the columns that describe a PBI's current state.

pinto does not require a server, account, or database service. A new board can
be initialized in the directory where the work is kept, and the same CLI can
manage it from the first item through completion.

This book is the task-oriented guide for users and contributors. Detailed
design decisions, JSON contracts, and migration notes remain in the repository
reference documents:

- [Design decisions](https://github.com/moriturus/pinto/blob/main/docs/DESIGN.md)
- [JSON output](https://github.com/moriturus/pinto/blob/main/docs/json-schema.md)
- [Storage migration](https://github.com/moriturus/pinto/blob/main/docs/migration.md)
