#!/usr/bin/env python3
"""Single dispatch, private credentials, durable pre-dispatch markers; no write retries."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
from urllib.parse import urlparse

READS = {"verify", "metadata", "list_roles", "public_key", "inspect_record", "inspect_operation", "verify_operation"}
IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}\Z")

def private(path):
    assert not path.is_symlink()
    metadata = path.stat()
    assert stat.S_ISREG(metadata.st_mode) and stat.S_IMODE(metadata.st_mode) == 0o600
    assert metadata.st_uid == os.getuid() and metadata.st_size <= 131072
    return path.read_text()

def write_new(path, value):
    content = value if isinstance(value, str) else json.dumps(value, indent=2) + "\n"
    descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    with os.fdopen(descriptor, "w") as file:
        file.write(content)
        file.flush()
        os.fsync(file.fileno())

def execute(root, action):
    profile = json.loads(private(root / "operator-profile.json"))
    assert set(profile) == {"url", "capability_file"}
    url = urlparse(profile["url"])
    assert not url.username and not url.password and not url.query and not url.fragment
    assert url.path == "/_qualification/owners"
    assert url.hostname and (url.scheme == "https" or url.scheme == "http" and url.hostname in {"127.0.0.1", "localhost", "::1"})
    assert isinstance(action, dict) and set(action) <= {"action_id", "owner", "operation", "parameters", "credential_file", "secret_output"}
    assert IDENTIFIER.fullmatch(action["action_id"])
    assert action["owner"] in {"auth", "access", "audit", "approval", "management"}
    def local_file(name):
        assert isinstance(name, str) and Path(name).name == name and name not in {".", ".."}
        return root / name
    request = {key: value for key, value in action.items() if key not in {"credential_file", "secret_output"}}
    if "credential_file" in action:
        request["credential"] = private(local_file(action["credential_file"])).strip()
    secret_output = local_file(action["secret_output"]) if "secret_output" in action else None
    assert (action["operation"] == "issue") == (secret_output is not None)
    capability = private(local_file(profile["capability_file"])).strip()
    assert len(capability) >= 32
    mutating = action["operation"] not in READS
    digest = hashlib.sha256(json.dumps(request, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    marker = root / (action["action_id"] + ".started.json")
    completed = root / (action["action_id"] + ".completed.json")
    if mutating:
        if completed.exists():
            receipt = json.loads(private(completed))
            assert receipt["request_sha256"] == digest
            return {"state": "completed_without_dispatch", "secret_saved": False}
        if secret_output is not None:
            assert not secret_output.exists()
        # An interrupted dispatch cannot be replayed after a process or Worker restart.
        write_new(marker, {"owner": action["owner"], "operation": action["operation"], "request_sha256": digest})
    envelope = json.dumps({"url": profile["url"], "capability": capability, "request": request})
    result = subprocess.run(["node", str(Path(__file__).with_name("http.mjs"))], input=envelope, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=40, check=False)
    try:
        reply = json.loads(result.stdout)
    except (ValueError, TypeError):
        return {"state": "unknown" if mutating else "unavailable", "secret_saved": False}
    if result.returncode or reply.get("status") != 200 or reply.get("body", {}).get("state") != "completed":
        return {"state": "unknown" if mutating else "unavailable", "secret_saved": False}
    owner_reply = reply["body"]["result"]
    assert isinstance(owner_reply, dict)
    secret = owner_reply.pop("secret", None)
    if secret_output is not None:
        assert isinstance(secret, str) and 16 <= len(secret) <= 8192
        write_new(secret_output, secret + "\n")
    else:
        assert secret is None
    receipt = {"schema_version": 1, "layer": "supplemental-owner-operator", "ordinary_source_app": False,
               "owner": action["owner"], "operation": action["operation"], "request_sha256": digest,
               "state": "completed", "result": owner_reply, "secret_saved": secret_output is not None}
    if mutating:
        write_new(completed, receipt)
    else:
        output = root / (action["action_id"] + ".read.json")
        write_new(output, receipt)
    return {"state": "completed", "secret_saved": secret_output is not None}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--private-root", type=Path, required=True)
    parser.add_argument("--action", type=Path, required=True)
    options = parser.parse_args()
    root = options.private_root.resolve()
    assert root.is_dir() and not options.private_root.is_symlink()
    assert root.stat().st_uid == os.getuid() and stat.S_IMODE(root.stat().st_mode) == 0o700
    try:
        outcome = execute(root, json.loads(private(options.action)))
    except (AssertionError, FileExistsError, OSError, subprocess.TimeoutExpired, KeyError, ValueError):
        outcome = {"state": "stopped_without_replay", "secret_saved": False}
    print(json.dumps(outcome))
    raise SystemExit(0 if outcome["state"].startswith("completed") else 1)

if __name__ == "__main__":
    main()
