# RefreshAgent

Autonomous SEO improvements using your existing local Codex or Claude Code.
A Rust terminal interface and headless worker for macOS and Linux.

**Works independently. Optional RefreshAgent Cloud data improves prioritization.**
No RefreshAgent account is required for local scanning, execution, scheduling or history.
Your installed agent handles authentication and model usage under its own plan.

## Install

No Rust compiler is needed. macOS and Linux, Intel/x86_64 and ARM64:

```sh
curl -fsSL https://raw.githubusercontent.com/RefreshAgent/refreshagent/main/install.sh | sh
```

The installer verifies the release checksum and executable version, installs to
`~/.local/bin` without sudo, and adds that directory to your bash/zsh/sh profile.
Open a new terminal, then run `refreshagent` inside your website's Git repository.
Automatic binary updates are built in. The canonical script is
[install.sh](https://github.com/RefreshAgent/refreshagent/blob/main/install.sh).

### Homebrew

```sh
brew tap RefreshAgent/tap https://github.com/RefreshAgent/refreshagent
brew install RefreshAgent/tap/refreshagent
```

Use `brew upgrade refreshagent` for Homebrew installations. Each stable release
refreshes the formula automatically. The explicit tap URL keeps the formula in
this OSS repository.

### Manual download and prerequisites

Download the matching binary and `.sha256` file from
[GitHub Releases](https://github.com/RefreshAgent/refreshagent/releases/latest),
verify the SHA-256, make it executable, and place it on your PATH.
Git is required for repository work. Execution also requires an authenticated
`codex` or `claude` installation; local scanning does not require either agent.
`gh` is additionally required for pull-request delivery.

Installer options: set `REFRESHAGENT_VERSION=vX.Y.Z` to pin a stable release,
`REFRESHAGENT_INSTALL_DIR=/absolute/path` to change the directory, or
`REFRESHAGENT_NO_MODIFY_PATH=1` to manage PATH yourself. With a piped installer,
pass these variables to `sh`, for example:

```sh
curl -fsSL https://raw.githubusercontent.com/RefreshAgent/refreshagent/main/install.sh | REFRESHAGENT_VERSION=v0.1.2 sh
```

Pinning the installer does not disable subsequent automatic updates; use
`refreshagent update disable` if you need to stay on that version.

Onboarding discovers the framework and likely content folders, then asks for the
site URL, content source, writable roots, agent, validation command, delivery
policy and schedule. Review is the default delivery policy.

## Headless use

```sh
refreshagent init --yes --site https://example.com \
  --roots content,src/pages --agent codex --validation 'npm run build'
refreshagent doctor
refreshagent scan
refreshagent run --dry-run
refreshagent run
refreshagent history
```

`run --opportunity ID` selects a specific scan result. Otherwise the worker picks
the highest-priority eligible finding that has not already been handled at the
same content fingerprint. The scanner currently checks tracked Markdown/MDX for
missing titles/descriptions, duplicate titles, and empty Markdown/HTML links.
These are candidate findings: the local agent must verify template behavior
before editing. Scores are transparent local heuristics, not ranking factors.

Runs use isolated Git worktrees under the repository's Git metadata directory.
The original checkout remains available. Run history contains the worktree,
branch, task, event log, validation logs and diff. Inspect those logs with normal
file tools; they may contain private content and agent output.

At least one configured validation command is required before editing. Commands
run in the isolated worktree. Configure dependency preparation as part of the
command if needed, for example `npm ci && npm run build`. The time budget covers
agent execution and validation. Existing tracked edits block a run.

## Autonomous operation

```sh
refreshagent service install --preview
refreshagent service install
refreshagent service status
refreshagent pause
refreshagent resume
refreshagent service uninstall
```

Installation is explicit and creates a **user** systemd timer on Linux or a
LaunchAgent on macOS. It captures executable/project paths and PATH, but does not
copy API keys or credentials. Install the binary to a stable location first.
Each service check runs `refreshagent tick`, which executes at most one task
when the persisted interval is due. One repository lock prevents overlap.

Schedules pause after repeated failures, on interrupted runs, and while validated
work awaits review. `pause` stops future work; it does not cancel an active run.
`q` in the TUI cancels its active child process group. Service uninstall stops
the service. Timeouts stop child process groups and preserve partial work.

User services run while the user's session/environment is available. Linux
execution after logout may require separately configured user lingering. Sleeping
or powered-off machines cannot work; the next service check evaluates overdue
work after the machine becomes available. No root service or sleep prevention is
installed.

## Review and delivery

Edit `.refreshagent/config.toml` to choose:

- `delivery = "review"`: retain the validated diff on its isolated branch.
- `delivery = "commit"`: commit on the isolated local branch.
- `delivery = "pull_request"`: commit, push that branch to `origin`, and create a PR using `gh`.

No mode automatically merges, writes to the original branch, or publishes CMS
content. An existing deployment may run when a PR is merged externally.
After inspecting a run, use `refreshagent recover RUN_ID --status accepted` or
`--status dismissed` to acknowledge it and resume scheduling. This does not merge,
reset or delete the worktree. Recovery also acknowledges interrupted/failed runs.

## Optional Cloud data

The adapter uses RefreshAgent's existing managed Search Console API. Connect
Google and obtain an account API key at https://refreshagent.com. Provide explicit
URL-to-file mappings, because a public URL cannot reliably be guessed from a
framework's source filename:

```json
{
  "https://example.com/guides/snorkelling": "content/guides/snorkelling.md"
}
```

```sh
refreshagent cloud connect --property https://example.com/ --mapping page-map.json
# Set REFRESHAGENT_API_KEY through your shell/environment secret handling.
refreshagent cloud sync
refreshagent scan
```

Sync reads a 30-day period ending three days ago, preserving the previous snapshot
on HTTP errors or unmapped results. It never submits source files or prompts to
Cloud. Repeat `cloud sync` to refresh; this release does not automatically refresh
Cloud data in the service. Device login, remote queues and cross-machine history
are future work, not implemented capabilities.

You can also bring data from any provider by setting `evidence_file` to a JSON
array of `{ "path": "content/page.md", "clicks": 20, "impressions": 2000,
"position": 12, "period_end": "2026-10-01" }`. Performance data enriches existing
local findings; it does not yet independently detect content decay.

## Scope and limitations

v0.1 executes **repository content only**. API and mixed surfaces can be recorded
during onboarding, but execution fails explicitly until those adapters exist.
Bundled information-gain and computed-knowledge guidance informs the user's
agent; the worker does not claim a measured competitor novelty score without
an evidence corpus. It never invents traffic outcomes or causal attribution.

The wrapper checks proposed diffs and controls its own delivery steps. It is not
a security sandbox for arbitrary coding agents or repository build commands.
Codex uses workspace-write sandboxing; Claude uses its normal permission system
with acceptEdits. Preconfigured agent tools, hooks and MCP connections retain
their own capabilities. Run only in trusted projects with appropriate agent
permissions. Claude commands requiring further permission may be denied in
headless execution and will need local configuration.

No telemetry is sent by the standalone worker. Optional Cloud requests follow
that service's account and quota policies. Config and data are ignored locally;
if your site's repository has different ignore rules, exclude `.refreshagent/`.

## Development

Contributors can build from source with stable Rust:

```sh
git clone https://github.com/RefreshAgent/refreshagent.git
cd refreshagent
cargo install --path . --locked
```

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
sh -n install.sh
python3 -m unittest discover -s tests -p 'test_*.py'
```

Apache-2.0. Contributions welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Built-in updates

Installed binaries automatically check for a newer stable GitHub release once a
day when starting the TUI, a manual run, or a scheduled tick. The check runs before
work starts. An installation lock defers replacement while any worker using that
binary is active. A running TUI does not check in the middle of a task.

```sh
refreshagent update check
refreshagent update
refreshagent update disable
refreshagent update enable
refreshagent update rollback
```

These commands work outside a site repository. `REFRESHAGENT_NO_UPDATE=1` disables
automatic checks for one invocation. Settings live in the user's configuration
directory, independently of site and Cloud configuration. Automatic failures
back off for a day and leave the installed version usable.

Updates download the matching macOS Intel/Apple Silicon or Linux x86_64/ARM64
binary from this repository's stable GitHub releases. The updater checks the
SHA-256 file (and GitHub asset digest when supplied), verifies the version with a
bounded startup check, then atomically replaces the binary at the same path.
It retains `refreshagent.previous` beside the executable for rollback. Rollback
disables automatic updates until explicitly re-enabled. Configurations, agent
credentials, run histories and service paths are preserved.

Development builds under `target/`, Homebrew Cellar and Nix-store installations
are not self-replaced; use their build/package manager. The updater never invokes
sudo. The install directory must be writable. Release checks/downloads trust
GitHub and repository release maintainers; checksums detect corruption and are
not an independent signing system.

Maintainers: bump Cargo.toml/Cargo.lock and push to `main`. The release workflow
builds/tests all four platforms, then creates the matching stable `vX.Y.Z` tag
and publishes binaries/checksums, then refreshes the Homebrew formula. Already published versions are skipped.
Pushing a matching version tag or dispatching a run for an existing tag also
works. Draft releases can be resumed; published assets are never overwritten.
Linux x86_64 binaries require Ubuntu 22.04-era glibc or newer; ARM64 binaries
require Ubuntu 24.04-era glibc or newer. macOS compatibility follows the build
runner's SDK/deployment target. An incompatible candidate fails its startup check
and leaves the installed binary untouched. No update is installed until a release
with matching assets is published.
