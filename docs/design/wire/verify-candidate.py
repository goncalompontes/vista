#!/usr/bin/env python3
"""Compile the non-canonical candidate and check its hand-authored vectors."""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
SCHEMA_DIR = Path(__file__).resolve().parent
SCHEMA = "vista-v1.draft.proto"
PROTOC = sys.argv[1] if len(sys.argv) > 1 else "protoc"

subprocess.run([PROTOC, "--version"], check=True)
(ROOT / "target").mkdir(exist_ok=True)
subprocess.run([
    PROTOC, f"-I{SCHEMA_DIR}", f"--descriptor_set_out={ROOT / 'target/vista-draft.pb'}",
    SCHEMA,
], check=True)
fixture = json.loads((SCHEMA_DIR / "golden-v1.json").read_text())
for vector in fixture["vectors"]:
    result = subprocess.run([
        PROTOC, f"-I{SCHEMA_DIR}", f"--decode={vector['type']}", SCHEMA,
    ], input=bytes.fromhex(vector["hex"]), stdout=subprocess.PIPE, check=True)
    if not result.stdout:
        raise SystemExit(f"empty decode output for {vector['id']}")
print(f"compiled candidate schema and decoded {len(fixture['vectors'])} golden vectors")
