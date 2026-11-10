#!/usr/bin/env python3
"""Compile and run JavaLite conformance tests; not an Android-device test."""
from pathlib import Path
from tools import HERE, java_runtime, run

runtime = java_runtime()
source = HERE / 'generated/java/dev/vista/candidate/v1/Vista.java'
test = HERE / 'android/CandidateWireTest.java'
out = HERE / 'generated/jvm-classes'
out.mkdir(parents=True, exist_ok=True)
run('javac', '-cp', runtime, '-d', out, source, test)
run('java', '-cp', f'{out}:{runtime}', 'CandidateWireTest', HERE / 'fixtures/golden.tsv')
