"""Prepare explicit Host approval inputs; Engine verification remains authoritative."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile


PLUGIN_ID = "lenso.reference.knowledge-settings"
WORLD = "lenso:knowledge-settings-local@1.0.0/plugin"
FILES = ("README.md", "knowledge-settings-local.mjs", "worker.mjs")


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def no_symlinks(path):
    path = Path(os.path.abspath(path))
    for part in (path, *path.parents):
        if part.is_symlink():
            raise ValueError(f"symlinks are not accepted: {part}")
    return path


def read_regular(path, limit):
    path = no_symlinks(path)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        metadata = os.fstat(fd)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError(f"expected bounded regular file: {path}")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read(limit + 1)
        if len(data) > limit:
            raise ValueError(f"file exceeds bound: {path}")
        return data
    finally:
        os.close(fd)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate manifest key: {key}")
        result[key] = value
    return result


def reject_manifest_number(value):
    raise ValueError(f"floating-point manifest numbers are not supported: {value}")


def prepare(manifest_path, component_path, output):
    manifest_bytes = read_regular(manifest_path, 1_048_576)
    manifest = json.loads(
        manifest_bytes, object_pairs_hook=unique_object,
        parse_float=reject_manifest_number, parse_constant=reject_manifest_number,
    )
    canonical = json.dumps(
        manifest, sort_keys=True, separators=(",", ":"), ensure_ascii=False,
        allow_nan=False,
    ).encode()
    if not isinstance(manifest, dict) or not isinstance(manifest.get("contract", manifest), dict):
        raise ValueError("expected a Bundle manifest object")
    identity = manifest.get("contract", manifest).get("plugin_id")
    if identity != PLUGIN_ID:
        raise ValueError("manifest is not the knowledge-settings Plugin")
    component = read_regular(component_path, 64 * 1024 * 1024)
    if component[:8] != b"\0asm\x0d\0\x01\0":
        raise ValueError("expected a WebAssembly Component")
    sources = {
        name: read_regular(Path(__file__).parent / name, 1_048_576)
        for name in FILES
    }
    profile = {
        "schema": "lenso.workers-integration.v1",
        "plugin_id": PLUGIN_ID,
        "instance_key": "default",
        "authoring_version": 2,
        "world": WORLD,
        "manifest_digest": digest(canonical),
        "artifact_digest": digest(component),
        "runtime_version": "0.1.5",
        "files": {name: digest(data) for name, data in sources.items()},
    }
    encoded = json.dumps(profile, sort_keys=True, separators=(",", ":")).encode()
    output = no_symlinks(output)
    if output.exists():
        raise FileExistsError(output)
    # Stage completely before reserving the destination. mkdir refuses even an
    # empty existing directory; rename alone would silently replace one on Unix.
    with tempfile.TemporaryDirectory(prefix=".workers-integration-", dir=output.parent) as temporary:
        staged = Path(temporary) / "ready"
        staged.mkdir()
        for name, data in {**sources, "integration.json": encoded}.items():
            (staged / name).write_bytes(data)
        output.mkdir()
        try:
            os.replace(staged, output)
        except BaseException:
            output.rmdir()
            raise
    return output / "integration.json", digest(encoded)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--component", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    path, profile_digest = prepare(args.manifest, args.component, args.output)
    print(json.dumps({"profile": str(path), "profile_digest": profile_digest}))


if __name__ == "__main__":
    main()
