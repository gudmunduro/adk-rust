#!/usr/bin/env python3
"""CI check: documentation version snippets and feature names must be real.

Two classes of doc rot this catches (both shipped in the past):
  1. Stale versions — install snippets like `adk-rust = "0.8.2"` surviving a
     release (the docs.rs landing page advertised 0.8.2 while 1.0.0 was out).
  2. Phantom features — docs telling users to enable a feature that does not
     exist in adk-rust/Cargo.toml (the `labs` feature was documented for
     months after it was removed).

Checks every tracked *.md and *.rs file, excluding CHANGELOG.md and
historical/third-party content (reference/, docs/podcast/, learning/, etc.).
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

SKIP_PARTS = {"reference", "learning", "tmp", "output", "target", ".git", "proptest-regressions"}


def workspace_version() -> str:
    in_pkg = False
    for line in (ROOT / "Cargo.toml").read_text().splitlines():
        if line.strip().startswith("["):
            in_pkg = line.strip() == "[workspace.package]"
        elif in_pkg:
            m = re.match(r'version = "([^"]+)"', line.strip())
            if m:
                return m.group(1)
    sys.exit("error: could not find workspace version")


def adk_rust_features() -> set[str]:
    text = (ROOT / "adk-rust" / "Cargo.toml").read_text()
    m = re.search(r"^\[features\]$(.*?)^\[", text, re.M | re.S)
    if not m:
        sys.exit("error: could not find [features] in adk-rust/Cargo.toml")
    features = set()
    for line in m.group(1).splitlines():
        fm = re.match(r"^([A-Za-z0-9_-]+)\s*=", line.strip())
        if fm:
            features.add(fm.group(1))
    return features


def skip(rel: Path) -> bool:
    if any(part in SKIP_PARTS for part in rel.parts):
        return True
    if rel.parts[:2] == ("docs", "podcast"):
        return True
    return rel.name == "CHANGELOG.md"


def main() -> None:
    version = workspace_version()
    features = adk_rust_features()
    errors: list[str] = []

    version_pattern = re.compile(
        r'\b(?:adk|awp|cargo)-[a-z0-9-]+ = (?:"(\d+\.\d+\.\d+)"|\{ version = "(\d+\.\d+\.\d+)")'
    )
    # `adk-rust = { version = "...", features = [...] }` or
    # `adk-rust = { ... features = ["a", "b"] }` on one line
    adk_rust_features_pattern = re.compile(
        r'\badk-rust = \{[^}]*features = \[([^\]]*)\]'
    )

    tracked = subprocess.run(
        ["git", "ls-files", "*.md", "*.rs"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.splitlines()

    for rel_str in tracked:
        rel = Path(rel_str)
        path = ROOT / rel
        if skip(rel) or not path.exists():
            continue
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            for m in version_pattern.finditer(line):
                found = m.group(1) or m.group(2)
                if found != version:
                    errors.append(
                        f"{rel}:{lineno}: version '{found}' != workspace '{version}': {line.strip()}"
                    )
            for m in adk_rust_features_pattern.finditer(line):
                for feat in re.findall(r'"([^"]+)"', m.group(1)):
                    if feat not in features:
                        errors.append(
                            f"{rel}:{lineno}: feature '{feat}' does not exist in adk-rust/Cargo.toml"
                        )

    if errors:
        print(f"check-doc-versions: {len(errors)} problem(s) found "
              f"(workspace version: {version}):\n", file=sys.stderr)
        for e in errors:
            print(f"  {e}", file=sys.stderr)
        print("\nRun `bash scripts/bump-version.sh <version>` to update version "
              "snippets, or fix the feature names by hand.", file=sys.stderr)
        sys.exit(1)

    print(f"check-doc-versions: OK (version {version}, {len(features)} features)")


if __name__ == "__main__":
    main()
