# RefreshAgent

Autonomous SEO improvements using your existing local Codex or Claude Code.
A Rust terminal interface and headless worker for macOS and Linux.

**Works independently. Optional RefreshAgent Cloud data improves prioritization.**
No RefreshAgent account is required for local scanning, execution, scheduling or history.
Your installed agent handles authentication and model usage under its own plan.

## Install from source

```sh
git clone https://github.com/RefreshAgent/refreshagent.git
cd refreshagent
cargo install --path . --locked
cd /path/to/your/website
refreshagent
```

Rust stable, Git, and an authenticated `codex` or `claude` installation are required.
`gh` is additionally required for pull-request delivery. This initial release is
source-installable; it is not yet published to crates.io or Homebrew.

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

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

Apache-2.0. Contributions welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).
