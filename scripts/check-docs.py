#!/usr/bin/env python3
"""Check public local Markdown links and compile/run the two entry-point guides."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parent.parent


def prose(path):
    """Omit fenced examples: their strings are not documentation links."""
    lines = []
    fence = None
    for line in path.read_text(encoding="utf-8").splitlines():
        marker = re.match(r"^\s*(`{3,}|~{3,})", line)
        if marker:
            token = marker.group(1)
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            lines.append("")
        else:
            lines.append(line if fence is None else "")
    return lines


def heading_ids(path):
    ids = set()
    counts = {}
    for line in prose(path):
        match = re.match(r"^#{1,6}\s+(.+?)\s*#*\s*$", line)
        if not match:
            continue
        heading = match.group(1).lower()
        base = re.sub(r"[^\w\- ]", "", heading).replace(" ", "-")
        count = counts.get(base, 0)
        counts[base] = count + 1
        ids.add(f"{base}-{count}" if count else base)
    return ids


def check_links():
    files = [ROOT / "README.md", ROOT / "CHANGELOG.md"]
    files.extend(sorted((ROOT / "DOCUMENTATION").rglob("*.md")))
    files.extend(sorted((ROOT / "tests/assets").rglob("*.md")))
    files.extend(sorted((ROOT / "src/rendering/easter_eggs/assets").rglob("*.md")))
    failures = []
    for path in files:
        for number, line in enumerate(prose(path), 1):
            for match in re.finditer(r"\[[^\]]*\]\(([^\s)]+)(?:\s+\"[^\"]*\")?\)", line):
                link = match.group(1).strip("<>")
                parts = urlsplit(link)
                if parts.scheme or parts.netloc:
                    continue
                target = (path.parent / unquote(parts.path)).resolve() if parts.path else path
                problem = None
                if not target.exists():
                    problem = "missing target"
                elif parts.fragment and target.suffix == ".md":
                    if unquote(parts.fragment) not in heading_ids(target):
                        problem = "missing heading"
                if problem:
                    failures.append(f"{path.relative_to(ROOT)}:{number}: {problem}: {link}")
    if failures:
        raise RuntimeError("\n".join(failures))
    print(f"Local documentation links: {len(files)} files passed.", flush=True)


def check_program(document, desktop):
    command = ["cargo", "build", "--locked", "--lib", "--no-default-features",
               "--message-format=json-render-diagnostics"]
    if desktop:
        command.extend(["--features", "desktop"])
    result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True)
    libraries = []
    for line in result.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message["target"]["name"] == "sim_logic":
            libraries.extend(Path(name) for name in message["filenames"] if name.endswith(".rlib"))
    if len(libraries) != 1:
        raise RuntimeError(f"Expected one sim_logic rlib, found {libraries}")
    library = libraries[0]
    # Derive macros inspect Cargo's manifest environment. Use the independent
    # consumer, whose only dependency is sim-logic, not this crate's own deps.
    environment = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / "tests/consumers/budget_api"))
    subprocess.run([
        "rustdoc", "--test", str(ROOT / document), "--edition=2024",
        "--extern", f"sim_logic={library}", "-L", f"dependency={library.parent}",
        "-L", f"dependency={library.parent / 'deps'}",
    ], cwd=ROOT, env=environment, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--examples", action="store_true", help="also check the exact Markdown Rust programs")
    args = parser.parse_args()
    check_links()
    if args.examples:
        check_program("DOCUMENTATION/Getting-Started.md", desktop=False)
        check_program("README.md", desktop=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
