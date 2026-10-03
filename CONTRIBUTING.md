# Contributing

Start with AGENTS.md and run formatting, clippy and tests before submitting a PR.
Changes must preserve standalone operation and keep Cloud optional. Avoid adding
separate model calls: use the configured agent for interpretation.

Useful next contributions: CMS draft adapters; richer evidence-backed scans;
normalized agent event presentation; optional OAuth device login; dated outcome
comparisons; release binaries. Keep adapters honest about their supported read,
preview, draft and publish operations.

Regression tests should exercise failure and delivery boundaries, not just mirror
implementation details. Never use live credentials in tests or commit user logs.
