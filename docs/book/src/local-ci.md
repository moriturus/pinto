# Reproducing CI locally with `nektos/act`

Use [`nektos/act`](https://nektosact.com/) to run a selected GitHub Actions job
before pushing. `act` needs a Docker-compatible engine for containerized Linux
runners. The `check` job is in `ci.yml`, while the release job is in
`release.yml`. The release job uses the GitHub-provided `GITHUB_TOKEN` with
`contents: write` to create the release, so inspect it with a dry run locally
instead of accidentally publishing a real release. Never commit a token or a
secret file. If a future job needs a secret, provide it through act's
`--secret-file` or `--secret` options from a path that is outside the
repository.

Install Docker Desktop or Docker Engine, install `act` using the
[official installation guide](https://nektosact.com/installation/index.html),
and verify both tools before running a job:

```bash
docker version
act --version
act -l
```

## macOS and Linux

The release job uses `ubuntu-latest`, so inspect only that job to reproduce the
release build and package path without creating a release:

```bash
act -n push -j release
```

On Apple Silicon, add `--container-architecture linux/amd64` if the selected
Linux image is not available for the host architecture.

The selected job runs the same commands used by CI:

```bash
cargo build --release --all-features --locked
./scripts/verify-package.sh
cargo install --path . --locked --root "$PWD/.tmp/pinto"
./scripts/extract-release-notes.sh "$GITHUB_REF_NAME"
```

The live GitHub run executes the final `gh release create` step with the
tag-matched notes and the workflow token. Local dry runs do not publish or
require a repository token.

To run only the Linux leg of the quality-check matrix, select its matrix value
explicitly:

```bash
act push -j check --matrix os:ubuntu-latest
```

`act` uses Docker containers for these Linux jobs. It is useful for fast
feedback, but GitHub-hosted runner parity remains the responsibility of the
real CI job. The Windows matrix leg remains a host-executed validation and is
not emulated by a Linux Docker container.

## Windows

On a Windows host, select only the Windows matrix entry and map the runner to
the host instead of a Docker image:

```powershell
act push -j check --matrix os:windows-latest -P windows-latest=-self-hosted
```

This runs the Windows leg on the local Windows machine. It is act's
self-hosted-host mode, not a Windows Docker guest, so the machine must already
have a real Git checkout (including `.git`) and the native tools used by the
workflow (`git`, Node.js, compatible `unzip`/`tar`/`gzip`, `mise`, Rust, and
`rustup`) plus the Windows shell environment expected by the steps. Git for
Windows provides these archive tools in `usr\bin`; make sure
`C:\Program Files\Git\usr\bin` is on `PATH`. It does not run the
Linux or macOS matrix entries. Use the macOS/Linux procedure above for the
containerized Linux release job.

The Docker Desktop Linux engine can run the containerized Linux jobs, but it
cannot run Windows containers in that mode. The command above deliberately
does not require a Windows Docker engine: it executes the Windows leg directly
on the Windows host.

## Troubleshooting

- `docker version` cannot reach the engine: start Docker Desktop or the Docker
  service, then retry `act -l`.
- `act -l` shows no `release` or `check` job: run it from the repository root
  and confirm the workflow is under `.github/workflows/`.
- `path ... not located inside a git repository`: use a real clone or checkout
  that contains `.git`; a copied source tree is not enough for act's ref and
  revision detection.
- `Cannot find: node in PATH`: install Node.js and open a new terminal before
  running the Windows job; JavaScript actions such as `jdx/mise-action` need it.
- `Cannot find: unzip`, `gzip`, or an archive/cache compatibility error: put
  `C:\Program Files\Git\usr\bin` on `PATH`. Git for Windows supplies the
  archive tools required by the actions used here; the old GnuWin32 `unzip`
  package cannot unpack some current `mise` archives.
- `PSSecurityException` or a message that script execution is disabled: allow
  scripts only for the current PowerShell process, then invoke `act`:

  ```powershell
  $env:PSExecutionPolicyPreference = "Bypass"
  $env:Path = "C:\Program Files\Git\usr\bin;$env:Path"
  act push -j check --matrix os:windows-latest -P windows-latest=-self-hosted
  ```

  This avoids changing the machine or user execution policy permanently.
- Use `act -n push -j release` to validate the containerized workflow without
  creating a job container, and add `--verbose` when a step's exit status needs
  more context. In the Windows `-self-hosted` mapping, host shell steps still
  execute, so use the command only when running those steps is acceptable.
- `--matrix` filters existing matrix values; it does not create a new runner
  platform. The Windows command above therefore requires the
  `windows-latest` entry already present in `.github/workflows/ci.yml`.

These commands select jobs at invocation time and do not modify the production
workflow. GitHub CI remains the authoritative cross-platform check.
