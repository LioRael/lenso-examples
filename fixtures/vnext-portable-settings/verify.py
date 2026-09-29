#!/usr/bin/env python3
"""Compile before using any database. Candidate patches exist only in a temp copy."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent
DEFAULT_FRAMEWORK_SHA = "6c88aeda06c5da7aed9318863fe2eba189b6c236"


def run(*command, **kwargs):
    subprocess.run(command, check=True, **kwargs)


def verify_framework(framework, revision):
    if re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise SystemExit("LENSO_FRAMEWORK_REVISION must be a full lowercase 40-character SHA")
    actual = subprocess.check_output(
        ["git", "-C", str(framework), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(
        ["git", "-C", str(framework), "status", "--porcelain", "--untracked-files=all"],
        text=True)
    if actual != revision or dirty:
        raise SystemExit(f"framework must be clean at {revision}; got {actual}")
    return actual


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["generate", "lock", "check", "build"])
    args = parser.parse_args()
    if args.mode == "generate":
        run("lenso-contract-codegen", "generate", "contract/capability.json",
            "--rust", "src/generated.rs", cwd=ROOT / "crates/settings-contract")
        return
    framework = Path(os.environ["LENSO_FRAMEWORK_ROOT"]).resolve()
    if not (framework / "crates/lenso-kernel/Cargo.toml").is_file():
        raise SystemExit("LENSO_FRAMEWORK_ROOT must identify the candidate Rust checkout")
    revision = os.environ.get("LENSO_FRAMEWORK_REVISION", DEFAULT_FRAMEWORK_SHA)
    verify_framework(framework, revision)
    if args.mode in ("check", "build"):
        run("node", "--test", str(ROOT / "test_body.mjs"))
    with tempfile.TemporaryDirectory(prefix="portable-settings-", dir=ROOT) as directory:
        project = Path(directory)
        for name in ["Cargo.toml", "crates", "plugins", "src"]:
            source = ROOT / name
            if source.is_dir():
                shutil.copytree(source, project / name)
            else:
                shutil.copy2(source, project / name)
        if (ROOT / "Cargo.lock").exists():
            shutil.copy2(ROOT / "Cargo.lock", project / "Cargo.lock")
        patches = ["\n[patch.crates-io]"]
        for manifest in sorted((framework / "crates").glob("*/Cargo.toml")):
            package = tomllib.loads(manifest.read_text()).get("package", {})
            if package.get("name", "").startswith("lenso"):
                patches.append(f'{json.dumps(package["name"])} = {{ path = {json.dumps(str(manifest.parent))} }}')
        with (project / "Cargo.toml").open("a") as manifest:
            manifest.write("\n".join(patches) + "\n")
        env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / ".target"))
        if args.mode == "lock":
            run("cargo", "generate-lockfile", cwd=project, env=env)
            actual = verify_framework(framework, revision)
            shutil.copy2(project / "Cargo.lock", ROOT / "Cargo.lock")
            print(f"lock generated: framework SHA {actual}", flush=True)
            return
        run("cargo", "test", "--locked", "--workspace", "--lib", cwd=project, env=env)
        run("cargo", "check", "--locked", "--workspace", "--target", "wasm32-unknown-unknown", "--lib", cwd=project, env=env)
        if args.mode == "build":
            run("cargo", "build", "--locked", "--bins", cwd=project, env=env)
            run("cargo", "build", "--locked", "--target", "wasm32-unknown-unknown", "--lib", cwd=project, env=env)
            run("wasm-bindgen", "--target", "web", "--out-dir", str(ROOT / ".wasm"),
                str(ROOT / ".target/wasm32-unknown-unknown/debug/portable_settings_fixture.wasm"))
        actual = verify_framework(framework, revision)
        print(f"{args.mode} passed: framework SHA {actual}", flush=True)


if __name__ == "__main__":
    main()
