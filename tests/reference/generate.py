"""Maintainer-only golden generation using the independent Python reference CLI.

Usage: python3 tests/reference/generate.py /path/to/rds-memmap
Rust tests consume the checked-in results and never run Python or this script.
"""

import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
FIXTURES = HERE.parent / "fixtures"

for case in json.loads((HERE / "cases.json").read_text()):
    command = [
        sys.argv[1], "export", str(FIXTURES / case["source"]),
        "--source-root", str(FIXTURES), "--format", "markdown",
        "--view", case["view"], "--anchor-prefix", case["prefix"],
        "--output", str(HERE / (case["name"] + ".md")),
    ]
    for key, flag in [("map", "--map"), ("address_space", "--address-space")]:
        if key in case:
            command.extend([flag, case[key]])
    subprocess.run(command, check=True)

for name, source, space in [
    ("release.lock", "systems/mosaic_example.yaml", "cpu"),
    ("rendering.lock", "systems/rendering.yaml", "wide"),
]:
    subprocess.run([
        sys.argv[1], "lock", str(FIXTURES / source), "--source-root", str(FIXTURES),
        "--address-space", space, "--release", "native-compat-1",
        "--output", str(HERE / (name + ".json")),
    ], check=True)

combined = json.loads((HERE / "release.lock.json").read_text())
extra = json.loads((HERE / "rendering.lock.json").read_text())
combined["sources"].extend(extra["sources"])
combined["sources"].sort(key=lambda pin: pin["path"])
(HERE / "combined.lock.json").write_text(json.dumps(combined, indent=2, sort_keys=True) + "\n")
