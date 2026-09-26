#!/usr/bin/env python3
"""Inspect an already-built .crate archive without extracting it."""

import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile


ROOT = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"],
    cwd=ROOT, text=True,
))
package = next(item for item in metadata["packages"] if item["name"] == "sim-logic")
prefix = f"{package['name']}-{package['version']}"
archive = Path(metadata["target_directory"]) / "package" / f"{prefix}.crate"
with tarfile.open(archive, "r:gz") as tar:
    entries = tar.getmembers()

names = set()
for entry in entries:
    path = PurePosixPath(entry.name)
    if path.is_absolute() or ".." in path.parts or path.parts[0] != prefix:
        raise SystemExit(f"Unexpected archive path: {entry.name}")
    relative = path.relative_to(prefix)
    if not entry.isfile():
        raise SystemExit(f"Unexpected non-file archive entry: {entry.name}")
    if any(part in {"Workflow", "TargetProdDocs", ".idea", ".git", "target", "__pycache__"}
           for part in relative.parts):
        raise SystemExit(f"Private/generated path in package: {relative}")
    if relative.name.startswith("voxel-sandbox") and ".save" in relative.name:
        raise SystemExit(f"Saved game in package: {relative}")
    names.add(str(relative))

required = {
    "Cargo.toml", "README.md", "CHANGELOG.md", "LICENSE-MIT", "LICENSE-APACHE",
    "src/lib.rs", "tests/assets/text/DejaVuSans.ttf", "tests/assets/text/LICENSE-DejaVu.txt",
    "src/rendering/easter_eggs/assets/ferris.png", "src/rendering/easter_eggs/assets/README.md",
}
if missing := required - names:
    raise SystemExit(f"Missing package files: {sorted(missing)}")
print(f"Package contents: {len(names)} files, {archive.stat().st_size} compressed bytes; "
      "required licenses/assets present, no private/generated paths.")
