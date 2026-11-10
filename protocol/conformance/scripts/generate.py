#!/usr/bin/env python3
"""Compiler-derived fixtures and SDKs, kept separate from production clients."""
import argparse
import json
import os
import shutil
from tools import CACHE, HERE, LOCK, ROOT, protoc, run


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--swift", action="store_true")
    args = parser.parse_args()
    compiler = protoc()
    generated = HERE / "generated"
    generated.mkdir(exist_ok=True)
    schema = "vista/v1/vista.proto"
    common = [compiler, f"-I{ROOT / 'proto'}"]
    run(*common, f"--descriptor_set_out={generated / 'vista-v1.bin'}", schema)
    # Prove the original PR #76 draft compiles and still has identical wire fields.
    run(compiler, f"-I{ROOT / 'docs/design/wire'}",
        f"--descriptor_set_out={CACHE / 'draft.bin'}", "vista-v1.draft.proto")
    draft = (ROOT / "docs/design/wire/vista-v1.draft.proto").read_text()
    candidate = (ROOT / "proto" / schema).read_text()
    import re
    def tokens(source):
        source = re.sub(r"//[^\n]*", "", source)
        source = re.sub(r"option java_\w+\s*=\s*[^;]+;", "", source)
        return re.sub(r"\s+", "", source)
    assert tokens(draft) == tokens(candidate), "candidate drift from PR #76 draft"
    fixtures = HERE / "fixtures"
    rows = []
    for item in json.loads((fixtures / "index.json").read_text()):
        binary = run(*common, f"--encode={item['type']}", schema,
                     input=(fixtures / f"{item['id']}.textproto").read_bytes(),
                     capture_output=True).stdout
        rows.append(f"{item['id']}\t{item['type']}\t{binary.hex()}")
    # New sender's optional field 99; old runtimes have different relay behavior.
    rows.append("unknown-field\tvista.v1.ProtocolVersion\t0801980602")
    # Encode a genuine additive v1.1 schema with protoc (same full message name).
    newer = HERE / "evolution/v1.1"
    binary = run(compiler, f"-I{newer}", "--encode=vista.v1.ControlHello", schema,
                 input=b'selected_version { major: 1 minor: 1 } experiment_note: "additive"',
                 capture_output=True).stdout
    rows.append(f"additive-v1.1\tvista.v1.ControlHello\t{binary.hex()}")
    (fixtures / "golden.tsv").write_text("\n".join(rows) + "\n")
    # Independently check the old hand-authored wire-shape fixtures with protoc.
    old = json.loads((ROOT / "docs/design/wire/golden-v1.json").read_text())
    for vector in old["vectors"]:
        run(*common, f"--decode={vector['type']}", schema,
            input=bytes.fromhex(vector["hex"]), capture_output=True)
    java = generated / "java"
    shutil.rmtree(java, ignore_errors=True)
    java.mkdir()
    run(*common, f"--java_out=lite:{java}", schema)
    env = dict(os.environ, PROTOC=str(compiler), VISTA_PROTO_REGENERATE="1")
    run("cargo", "build", "--locked", cwd=HERE / "rust", env=env)
    if args.swift:
        plugin = CACHE / "swift-protobuf"
        if not plugin.exists():
            run("git", "clone", "--branch", LOCK["swift-protobuf"]["version"], "--depth", "1",
                "https://github.com/apple/swift-protobuf.git", plugin)
        revision = run("git", "rev-parse", "HEAD", cwd=plugin, capture_output=True, text=True).stdout.strip()
        assert revision == LOCK["swift-protobuf"]["revision"]
        run("swift", "build", "-c", "release", "--product", "protoc-gen-swift", cwd=plugin)
        output = HERE / "swift/Sources/VistaCandidate"
        # protoc does not create the --swift_out directory on a fresh checkout.
        output.mkdir(parents=True, exist_ok=True)
        run(*common, f"--plugin=protoc-gen-swift={plugin / '.build/release/protoc-gen-swift'}",
            f"--swift_out={output}", "--swift_opt=Visibility=Public,FileNaming=DropPath", schema)


if __name__ == "__main__":
    main()
