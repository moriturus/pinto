# Self-hosting migration

pinto migrated its project backlog from the repository-level `backlog.md` into
its own `.pinto/` board. The legacy file is frozen as a historical snapshot;
all current planning work belongs in the self-hosted board.

To reproduce the workflow in another repository, initialize a board, create or
import PBIs through the CLI, verify the board with `pinto list` and `pinto
board`, and commit the resulting text files. Use normal Git history for audit
and recovery. Do not maintain a second manually synchronized backlog.
