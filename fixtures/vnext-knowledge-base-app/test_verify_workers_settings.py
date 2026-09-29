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
    json_module,
    local_origin,
    private_json,
    write_private_json,
)


def auth_token(byte):
    return "lenso_at_" + base64.urlsafe_b64encode(bytes([byte]) * 32).decode().rstrip("=")


class WorkersSettingsAcceptanceTests(unittest.TestCase):
    def test_generated_modules_must_be_bounded_regular_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "module.mjs"
            source.write_bytes(b"export default {};\n")
            link = root / "link.mjs"
            link.symlink_to(source)
            with self.assertRaises(OSError):
                json_module(link)
            with self.assertRaises(ValueError):
                json_module(root)
            source.write_bytes(b" " * (1_048_576 + 1))
            with self.assertRaises(ValueError):
                json_module(source)

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
                "guest.js": b"bindings",
                "component-admission.mjs": b"admission",
                "component-requests.mjs": b"requests",
                "knowledge-settings-local.mjs": b"adapter",
                "worker.mjs": b"worker",
                "README.md": b"readme",
            }
            digest = lambda value: "sha256:" + hashlib.sha256(value).hexdigest()
            artifact_metadata = {
                "world": "lenso:knowledge-settings-local@1.0.0/plugin",
                "digest": digest(files["guest.component.wasm"]),
            }
            module = lambda value: b"export default " + json.dumps(value).encode() + b";\n"
            files["artifact.mjs"] = module(artifact_metadata)
            plan = {"plugin_instances": [{
                "instance_key": "lenso.reference.knowledge-settings/default",
                "package_id": "lenso.reference.knowledge-settings",
                "package_revision": digest(files["guest.component.wasm"]),
                "authoring_version": 2,
            }]}
            files["plan.mjs"] = module(plan)
            for name, data in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            receipt = {
                "schema": "lenso.workers-app-build.v1",
                "environment": "local-workerd",
                "plugin_id": "lenso.reference.knowledge-settings",
                "manifest_digest": digest(b"manifest"),
                "integration": {
                    "profile_file": "workers-integration.json",
                    "world": "lenso:knowledge-settings-local@1.0.0/plugin",
                    "plugin_id": "lenso.reference.knowledge-settings",
                    "instance_key": "default",
                    "authoring_version": 2,
                    "manifest_digest": digest(b"manifest"),
                    "artifact_digest": digest(files["guest.component.wasm"]),
                    "runtime_version": "0.1.5",
                    "module_digests": {
                        name: digest(files[name]) for name in (
                            "worker.mjs", "README.md", "knowledge-settings-local.mjs")
                    },
                },
                "expected_descriptor_digests": {"lenso.http.endpoint@1": "sha256:701deedf705cb1a3b2f35fcae72f20ae85d46c6da6a008405a519018bbcdd3fe"},
                "bundle_digest": digest(files["bundles/lenso.reference.knowledge-settings.lenso-plugin"]),
                "component_digest": digest(files["guest.component.wasm"]),
                "jco_core_digest": digest(files["guest.core.wasm"]),
                "jco_bindings_digest": digest(files["guest.js"]),
                "plan_digest": digest(json.dumps(plan).encode()),
                "workers_runtime": {"version": "0.1.5", "module_digests": {
                    name: digest(files[name]) for name in (
                        "component-admission.mjs", "component-requests.mjs")
                }},
            }
            profile = {
                "schema": "lenso.workers-integration.v1",
                **{key: value for key, value in receipt["integration"].items()
                   if key not in ("profile_file", "module_digests")},
                "files": receipt["integration"]["module_digests"],
            }
            profile_bytes = json.dumps(profile).encode()
            (root / "workers-integration.json").write_bytes(profile_bytes)
            receipt["integration"]["profile_digest"] = digest(profile_bytes)
            path = root / "workers-build.json"
            path.write_text(json.dumps(receipt))
            self.assertEqual(checked_workers_build(path), digest(path.read_bytes()))
            for invalid in (
                b"",
                module({**artifact_metadata, "world": "wrong"}),
                module({**artifact_metadata, "digest": digest(b"wrong")}),
                module({**artifact_metadata, "extra": True}),
                files["artifact.mjs"] + b"console.log('unexpected');\n",
                b'export default {"world":"duplicate","world":"duplicate"};\n',
            ):
                (root / "artifact.mjs").write_bytes(invalid)
                with self.subTest(artifact=invalid), self.assertRaises(ValueError):
                    checked_workers_build(path)
            (root / "artifact.mjs").write_bytes(module(dict(reversed(list(artifact_metadata.items())))))
            self.assertEqual(checked_workers_build(path), digest(path.read_bytes()))
            (root / "artifact.mjs").write_bytes(files["artifact.mjs"])
            (root / "guest.js").write_bytes(b"changed bindings")
            with self.assertRaises(ValueError):
                checked_workers_build(path)
            (root / "guest.js").write_bytes(files["guest.js"])
            (root / "plan.mjs").write_bytes(module({"plugin_instances": []}))
            with self.assertRaises(ValueError):
                checked_workers_build(path)
            original_plan_digest = receipt["plan_digest"]
            for key in ("instance_key", "package_id", "package_revision", "authoring_version"):
                changed_plan = {"plugin_instances": [{**plan["plugin_instances"][0], key: "wrong"}]}
                (root / "plan.mjs").write_bytes(module(changed_plan))
                receipt["plan_digest"] = digest(json.dumps(changed_plan).encode())
                path.write_text(json.dumps(receipt))
                with self.subTest(plan_key=key), self.assertRaises(ValueError):
                    checked_workers_build(path)
            receipt["plan_digest"] = original_plan_digest
            path.write_text(json.dumps(receipt))
            (root / "plan.mjs").write_bytes(files["plan.mjs"])
            for key, value in (
                ("world", "wrong"),
                ("manifest_digest", "sha256:" + "z" * 64),
                ("artifact_digest", digest(b"wrong-component")),
                ("instance_key", "lenso.reference.knowledge-settings/other"),
            ):
                original = receipt["integration"][key]
                receipt["integration"][key] = value
                path.write_text(json.dumps(receipt))
                with self.subTest(key=key), self.assertRaises(ValueError):
                    checked_workers_build(path)
                receipt["integration"][key] = original
            receipt["workers_runtime"]["module_digests"]["knowledge-settings-local.mjs"] = digest(b"adapter")
            path.write_text(json.dumps(receipt))
            with self.assertRaises(ValueError):
                checked_workers_build(path)
            del receipt["workers_runtime"]["module_digests"]["knowledge-settings-local.mjs"]
            path.write_text(json.dumps(receipt))
            (root / "workers-integration.json").write_bytes(profile_bytes + b" ")
            with self.assertRaises(ValueError):
                checked_workers_build(path)
            changed_profile = {**profile, "world": "wrong"}
            changed_bytes = json.dumps(changed_profile).encode()
            (root / "workers-integration.json").write_bytes(changed_bytes)
            receipt["integration"]["profile_digest"] = digest(changed_bytes)
            path.write_text(json.dumps(receipt))
            with self.assertRaises(ValueError):
                checked_workers_build(path)
            (root / "workers-integration.json").write_bytes(profile_bytes)
            receipt["integration"]["profile_digest"] = digest(profile_bytes)
            path.write_text(json.dumps(receipt))
            (root / "artifact.mjs").unlink()
            with self.assertRaises(FileNotFoundError):
                checked_workers_build(path)
            (root / "artifact.mjs").write_bytes(files["artifact.mjs"])
            (root / "knowledge-settings-local.mjs").write_bytes(b"changed")
            with self.assertRaises(ValueError):
                checked_workers_build(path)


if __name__ == "__main__":
    unittest.main()
