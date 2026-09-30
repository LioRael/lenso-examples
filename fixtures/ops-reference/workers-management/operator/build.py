#!/usr/bin/env python3
"""Build the private operator with the repository's normal Cargo toolchain."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.parse import urlparse

root = Path(__file__).resolve().parent


def source_hashes():
    files = [root / name for name in ["Cargo.toml", "Cargo.lock", "worker.mjs", "build.py", "stage_assets.py"]]
    files.extend(sorted((root / "src").rglob("*.rs")))
    assert files and all(file.is_file() and not file.is_symlink() for file in files)
    return {str(file.relative_to(root)): hashlib.sha256(file.read_bytes()).hexdigest() for file in files}


inputs = source_hashes()
lock = tomllib.loads((root / "Cargo.lock").read_text())
audit_sources = [package["source"] for package in lock["package"]
                 if package["name"] == "lenso-audit-log-d1-plugin"]
assert len(audit_sources) == 1
audit_revision = urlparse(audit_sources[0].removeprefix("git+")).fragment
annotation = re.search(r'"audit_owner_source"\s*:\s*"([a-f0-9]{40})"',
                       (root / "src/audit_proof.rs").read_text())
assert annotation and annotation.group(1) == audit_revision, "Audit annotation differs from the compiled Owner"
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
assert source_hashes() == inputs, "Operator source changed during its build"
outputs = sorted(file for file in package.iterdir() if file.name != "build-receipt.json")
assert outputs and all(file.is_file() and not file.is_symlink() for file in outputs)
receipt = {"schema_version": 2, "layer": "private-owner-operator-build",
           "ordinary_source_app": False,
           "wasm_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
           "cargo_lock_sha256": inputs["Cargo.lock"],
           "source_files_sha256": inputs,
           "package_files_sha256": {file.name: hashlib.sha256(file.read_bytes()).hexdigest() for file in outputs}}
(package / "build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps({"state": "built", "wasm_sha256": receipt["wasm_sha256"]}))
