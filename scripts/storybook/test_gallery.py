#!/usr/bin/env python3
"""Compatibility entry: exercise the React workbench served by the local preview."""
from pathlib import Path
import subprocess
import sys
root=Path(__file__).resolve().parents[2]
subprocess.run(['uv','run',str(root/'apps/zork-design/scripts/check-ui.py'),'--base','http://127.0.0.1:49186/design/',*sys.argv[1:]],check=True)
