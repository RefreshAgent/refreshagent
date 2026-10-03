#!/usr/bin/env python3
"""Capture actual local scan/dry-run output for the public worker page."""
import argparse
import datetime
import json
import os
import pathlib
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, help="Path to a built RefreshAgent binary")
    parser.add_argument("--output", default="scan-demo.json")
    args = parser.parse_args()
    binary = str(pathlib.Path(args.binary).resolve())
    env = {**os.environ, "REFRESHAGENT_NO_UPDATE": "1"}
    with tempfile.TemporaryDirectory(prefix="refreshagent-demo-") as temporary:
        root = pathlib.Path(temporary)
        (root / "content").mkdir()
        (root / "content/walks.md").write_text(
            "# Coastal walking guide\n\nFind the [route map]().\n"
        )
        def run(argv):
            return subprocess.run(argv, cwd=root, env=env, check=True,
                                  capture_output=True, text=True).stdout
        run(["git", "init", "-q"])
        run(["git", "add", "content"])
        run(["git", "-c", "user.name=RefreshAgent Demo",
             "-c", "user.email=demo@example.invalid", "commit", "-qm", "Add demo fixture"])
        run([binary, "init", "--yes", "--site", "https://example.invalid",
             "--roots", "content", "--agent", "codex",
             "--validation", "test -s content/walks.md"])
        commands = []
        for command in [["scan"], ["run", "--dry-run"]]:
            commands.append({"command": "refreshagent " + " ".join(command),
                             "output": run([binary, *command]), "exit_code": 0})
        assert any(item["issue"] == "empty-link"
                   for item in json.loads(commands[0]["output"]))
        recording = {
            "version": run([binary, "--version"]).strip(),
            "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "fixture": "content/walks.md",
            "commands": commands,
            "limitation": "Actual local scan and dry-run output. No coding agent executed and no content was edited.",
        }
    output = pathlib.Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(recording, indent=2) + "\n")
    print(f"Recorded {recording['version']} to {output}")


if __name__ == "__main__":
    main()
