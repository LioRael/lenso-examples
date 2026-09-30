#!/usr/bin/env python3
"""Build the private operator with the repository's normal Cargo toolchain."""
import hashlib
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent
assert subprocess.check_output(["wasm-bindgen", "--version"], text=True).strip() == "wasm-bindgen 0.2.127"
result = subprocess.run([
    "cargo", "rustc", "--locked", "--manifest-path", str(root / "Cargo.toml"),
    "--target", "wasm32-unknown-unknown", "--release", "--message-format=json",
    "--", "-C", "link-arg=--export=__wasm_call_ctors",
], cwd=root, text=True, stdout=subprocess.PIPE, check=True)
messages = [json.loads(line) for line in result.stdout.splitlines() if line]
artifacts = [Path(filename) for message in messages
             if message.get("reason") == "compiler-artifact"
             and message["target"]["name"] == "lenso_ops_workers_owner_operator"
             for filename in message["filenames"] if filename.endswith(".wasm")]
assert len(artifacts) == 1
package = root / "pkg"
package.mkdir(exist_ok=True)
subprocess.run(["wasm-bindgen", str(artifacts[0]), "--target", "web",
                "--experimental-reset-state-function", "--out-dir", str(package)], check=True)
binary = package / "lenso_ops_workers_owner_operator_bg.wasm"
receipt = {"schema_version": 1, "layer": "private-owner-operator-build",
           "ordinary_source_app": False,
           "wasm_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
           "cargo_lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest()}
(package / "build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps({"state": "built", "wasm_sha256": receipt["wasm_sha256"]}))
