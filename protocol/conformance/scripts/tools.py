#!/usr/bin/env python3
"""Download only locked, checksum-verified tools; never use a global protoc."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / "protocol/conformance"
LOCK = json.loads((HERE / "toolchain.lock.json").read_text())
CACHE = Path(os.environ.get("VISTA_PROTO_CACHE", HERE / ".cache"))


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def download(url, dest, digest):
    dest.parent.mkdir(parents=True, exist_ok=True)
    if not dest.exists():
        with urllib.request.urlopen(url) as source:
            dest.write_bytes(source.read())
    if hashlib.sha256(dest.read_bytes()).hexdigest() != digest:
        raise RuntimeError(f"checksum mismatch: {dest}; remove it and retry")
    return dest


def protoc():
    key = f"{platform.system()}-{platform.machine()}"
    archive, digest = LOCK["protoc"]["archives"][key]
    version = LOCK["protoc"]["version"]
    path = CACHE / f"protoc-{version}"
    if not (path / "bin/protoc").exists():
        zipped = download(
            f"https://github.com/protocolbuffers/protobuf/releases/download/v{version}/protoc-{version}-{archive}.zip",
            CACHE / f"protoc-{version}.zip", digest)
        with zipfile.ZipFile(zipped) as source:
            source.extractall(path)
        (path / "bin/protoc").chmod(0o755)
    binary = path / "bin/protoc"
    result = run(binary, "--version", capture_output=True, text=True)
    assert result.stdout.strip() == f"libprotoc {version}"
    return binary


def buf():
    version = LOCK["buf"]["version"]
    key = f"{platform.system()}-{platform.machine()}"
    path = download(f"https://github.com/bufbuild/buf/releases/download/v{version}/buf-{key}",
                    CACHE / f"buf-{version}", LOCK["buf"]["sha256"][key])
    path.chmod(0o755)
    assert run(path, "--version", capture_output=True, text=True).stdout.strip() == version
    return path


def java_runtime():
    info = LOCK["protobuf-javalite"]
    version = info["version"]
    return download(f"https://repo.maven.apache.org/maven2/com/google/protobuf/protobuf-javalite/{version}/protobuf-javalite-{version}.jar",
                    CACHE / f"protobuf-javalite-{version}.jar", info["sha256"])


if __name__ == "__main__":
    print(protoc())
    print(buf())
    print(java_runtime())
