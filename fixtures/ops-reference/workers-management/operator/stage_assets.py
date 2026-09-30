#!/usr/bin/env python3
"""Copy only exact clean Owner assets; the input has source paths, never credentials."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ASSETS = {
    "auth": [("crates/lenso-auth-api-token-plugin/src/host_facilities/state.mjs", "auth-state.mjs")],
    "access": [("crates/lenso-access-control-d1-plugin/src/host_facilities/state.mjs", "access-state.mjs")],
    "audit": [("crates/lenso-audit-log-d1-plugin/src/host_facilities/store.mjs", "audit-store.mjs")],
    "approval": [("crates/lenso-business-approval-d1-plugin/src/host_facilities/store.mjs", "approval-store.mjs")],
    "management": [("plugins/management/crates/lenso-management-core/src/workers/journal.mjs", "management-store.mjs")],
}

def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sources", type=Path, required=True)
    options = parser.parse_args()
    sources = json.loads(options.sources.read_text())
    assert set(sources) == set(ASSETS) | {"runtime"}
    destination = Path(__file__).resolve().parent / "owner-assets"
    destination.mkdir(exist_ok=True)
    receipt = {"schema_version": 1, "layer": "private-owner-operator-assets", "ordinary_source_app": False, "owners": {}}
    for owner, source in sources.items():
        assert set(source) == {"root", "sha"}
        root = Path(source["root"]).resolve()
        sha = source["sha"]
        assert len(sha) == 40 and all(character in "0123456789abcdef" for character in sha)
        assert git(root, "rev-parse", "HEAD") == sha
        assert not git(root, "status", "--porcelain")
        assets = ASSETS.get(owner)
        if owner == "runtime":
            prefix = "packages/lenso-workers-runtime/"
            files = git(root, "ls-files", prefix).splitlines()
            assets = [(file, "workers-runtime/" + file.removeprefix(prefix))
                      for file in files if file.endswith(".mjs") and "/test/" not in file]
            assert assets
        files = []
        for path, target in assets:
            content = subprocess.check_output(["git", "-C", str(root), "show", f"{sha}:{path}"])
            output = destination / target
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes(content)
            files.append({"path": path, "staged_path": target, "sha256": hashlib.sha256(content).hexdigest()})
        receipt["owners"][owner] = {"sha": sha, "files": files}
    (destination / "sources.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"state": "staged", "owners": {owner: value["sha"] for owner, value in receipt["owners"].items()}}))

if __name__ == "__main__":
    main()
