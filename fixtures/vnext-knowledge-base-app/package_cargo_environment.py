"""Offline Cargo environment for package-only acceptance builds."""

import os
from pathlib import Path


def package_build_environment(home):
    if not os.environ.get("CARGO_HOME"):
        raise RuntimeError("--package-only requires an explicit sandbox-local CARGO_HOME")
    home.mkdir(parents=True, exist_ok=True)
    environment = {
        key: os.environ[key]
        for key in ("PATH", "CARGO_HOME", "RUSTUP_HOME")
        if key in os.environ
    }
    environment["HOME"] = str(home)
    environment["CARGO_NET_OFFLINE"] = "true"
    target_dir = os.environ.get("LENSO_REFERENCE_CARGO_TARGET_DIR")
    if target_dir:
        target = Path(target_dir)
        if not target.is_absolute() or target == Path("/") or target.is_symlink():
            raise RuntimeError("LENSO_REFERENCE_CARGO_TARGET_DIR must be an absolute non-symlink task target")
        target.mkdir(parents=True, exist_ok=True)
        environment["CARGO_TARGET_DIR"] = str(target)
    return environment
