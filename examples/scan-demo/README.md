# Reproduce the local scan demo

This runs RefreshAgent against a temporary Git repository containing a Markdown
page with an empty link and no frontmatter description. It records real scan and
task-preview output. No coding agent is executed, no Cloud account is used, and
no website content is edited or published.

Prerequisites: an installed RefreshAgent binary, Git, and Python 3.

```sh
python3 examples/scan-demo/record.py \
  --binary "$(command -v refreshagent)" \
  --output scan-demo.json
```

The temporary repository is removed after capture. The JSON contains the binary
version, recording time, fixture filename, command output and limitations.
To run the flow on your own website, complete onboarding, then use
`refreshagent scan` and `refreshagent run --dry-run`.

A dry run previews the task passed to your agent. It does not demonstrate agent
execution, validation success, delivered changes or traffic outcomes.
