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
