"""Bounded local-workerd settings handoff and build-receipt checks."""

import base64
import hashlib
import json
import stat
import tempfile
import unittest
from pathlib import Path

from verify_workers_settings import (
    checked_workers_build,
    create_bridge_policy,
    handoff,
    local_origin,
    private_json,
    write_private_json,
)


def auth_token(byte):
    return "lenso_at_" + base64.urlsafe_b64encode(bytes([byte]) * 32).decode().rstrip("=")


class WorkersSettingsAcceptanceTests(unittest.TestCase):
    def test_handoff_requires_two_auth_token_shapes_and_loopback(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "handoff.json"
            tokens = {"user-a": auth_token(1), "user-b": auth_token(2)}
            write_private_json(path, {"token": tokens, "url": "http://127.0.0.1:4321"})
            self.assertEqual(handoff(path), ("http://127.0.0.1:4321", tokens))
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            for origin in ("http://localhost:4321", "http://0.0.0.0:4321", "https://127.0.0.1:4321"):
                with self.assertRaises(ValueError):
                    local_origin(origin)
            path.chmod(0o644)
            with self.assertRaises(ValueError):
                handoff(path)

    def test_bridge_policy_contains_only_short_lived_token_digests(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "policy.json"
            tokens = {"user-a": auth_token(1), "user-b": auth_token(2)}
            create_bridge_policy(path, tokens, 30)
            policy = private_json(path)
            self.assertEqual(policy["schema"], "lenso.knowledge-settings-auth-policy.v1")
            self.assertEqual([actor["owner_id"] for actor in policy["actors"]], ["user-a", "user-b"])
            self.assertEqual(policy["actors"][0]["token_sha256"],
                             "sha256:" + hashlib.sha256(tokens["user-a"].encode()).hexdigest())
            self.assertNotIn(tokens["user-a"], path.read_text())
            with self.assertRaises(FileExistsError):
                create_bridge_policy(path, tokens, 30)
            with self.assertRaises(ValueError):
                create_bridge_policy(Path(directory) / "long.json", tokens, 3601)

    def test_workers_receipt_rechecks_artifact_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = {
                "bundles/lenso.reference.knowledge-settings.lenso-plugin": b"bundle",
                "guest.component.wasm": b"component",
                "guest.core.wasm": b"core",
                "component-admission.mjs": b"admission",
                "component-requests.mjs": b"requests",
                "knowledge-settings-local.mjs": b"adapter",
            }
            digest = lambda value: "sha256:" + hashlib.sha256(value).hexdigest()
            for name, data in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            receipt = {
                "schema": "lenso.workers-app-build.v1",
                "environment": "local-workerd",
                "plugin_id": "lenso.reference.knowledge-settings",
                "private_world": "lenso:knowledge-settings-local@1.0.0/plugin",
                "host_bridge": "local-loopback-knowledge-settings.v1",
                "expected_descriptor_digests": {"lenso.http.endpoint@1": "sha256:701deedf705cb1a3b2f35fcae72f20ae85d46c6da6a008405a519018bbcdd3fe"},
                "bundle_digest": digest(files["bundles/lenso.reference.knowledge-settings.lenso-plugin"]),
                "component_digest": digest(files["guest.component.wasm"]),
                "jco_core_digest": digest(files["guest.core.wasm"]),
                "workers_runtime": {"version": "0.1.5", "module_digests": {
                    name: digest(files[name]) for name in files if name.endswith(".mjs")
                }},
            }
            path = root / "workers-build.json"
            path.write_text(json.dumps(receipt))
            self.assertEqual(checked_workers_build(path), digest(path.read_bytes()))
            (root / "knowledge-settings-local.mjs").write_bytes(b"changed")
            with self.assertRaises(ValueError):
                checked_workers_build(path)


if __name__ == "__main__":
    unittest.main()
