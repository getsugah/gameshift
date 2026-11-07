#!/usr/bin/env python3
"""Audit trusted build artifacts; this is not a proof of runtime compatibility."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("artifacts", nargs="+", type=Path)
parser.add_argument("--maximum", default="2.41")
args = parser.parse_args()
maximum = tuple(map(int, args.maximum.split(".")))
records = []
for path in args.artifacts:
    output = subprocess.run(["readelf", "--version-info", str(path)], capture_output=True, text=True, check=True).stdout
    versions = sorted({tuple(map(int, version.split("."))) for version in re.findall(r"Name: GLIBC_([0-9.]+)", output)})
    required = max(versions, default=())
    records.append({"artifact": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "required_glibc": ".".join(map(str, required)) or None, "within_baseline": required <= maximum})
print(json.dumps({"maximum_glibc": args.maximum, "artifacts": records, "warning": "Contrôler aussi les dépendances transitives, symboles GTK et le lancement réel sur Debian 13."}, indent=2))
raise SystemExit(0 if all(record["within_baseline"] for record in records) else 1)
