import json
from pathlib import Path
import tempfile
import unittest

from prepare_integration import FILES, PLUGIN_ID, digest, prepare


class PreparationTests(unittest.TestCase):
    def inputs(self, root):
        manifest = root / "manifest.json"
        component = root / "plugin.component.wasm"
        manifest.write_bytes(json.dumps(
            {"schema_version": 4, "contract": {"plugin_id": PLUGIN_ID}},
            sort_keys=True, separators=(",", ":"),
        ).encode())
        component.write_bytes(b"\0asm\x0d\0\x01\0")
        return manifest, component

    def test_deterministic_profile_and_sealed_file_set(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            manifest, component = self.inputs(root)
            first, first_digest = prepare(manifest, component, root / "first")
            second, second_digest = prepare(manifest, component, root / "second")
            self.assertEqual(first.read_bytes(), second.read_bytes())
            self.assertEqual(first_digest, second_digest)
            self.assertEqual(first_digest, digest(first.read_bytes()))
            profile = json.loads(first.read_bytes())
            self.assertEqual(set(p.name for p in first.parent.iterdir()),
                             {*FILES, "integration.json"})
            self.assertEqual(profile["manifest_digest"], digest(manifest.read_bytes()))
            self.assertEqual(profile["artifact_digest"], digest(component.read_bytes()))
            for name, expected in profile["files"].items():
                self.assertEqual(digest((first.parent / name).read_bytes()), expected)
            with self.assertRaises(FileExistsError):
                prepare(manifest, component, root / "first")
            (root / "empty").mkdir()
            with self.assertRaises(FileExistsError):
                prepare(manifest, component, root / "empty")

    def test_rejects_ambiguous_numbers_duplicate_keys_wrong_plugin_and_core_wasm(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            manifest, component = self.inputs(root)
            original = manifest.read_bytes()
            for invalid in (
                original.replace(PLUGIN_ID.encode(), b"other"),
                b'{"contract":{},"contract":{}}',
                b'{"contract":{"plugin_id":"first","plugin_id":"second"}}',
                original.replace(b'"schema_version":4', b'"schema_version":4.0'),
                original.replace(b'"schema_version":4', b'"schema_version":NaN'),
                original.replace(b'"schema_version":4', b'"schema_version":Infinity'),
            ):
                manifest.write_bytes(invalid)
                with self.assertRaises(ValueError):
                    prepare(manifest, component, root / "output")
            manifest.write_bytes(original)
            component.write_bytes(b"\0asm\x01\0\0\0")
            with self.assertRaises(ValueError):
                prepare(manifest, component, root / "output")
            self.assertFalse((root / "output").exists())

    def test_pretty_struct_order_and_canonical_manifests_bind_identically(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            manifest, component = self.inputs(root)
            document = {
                "schema_version": 4,
                "contract": {
                    "plugin_id": PLUGIN_ID,
                    "release_version": "0.1.0",
                    "authoring_version": 2,
                    "provided_capabilities": [],
                    "required_capabilities": [],
                },
                "implementations": [{
                    "id": "component",
                    "host_targets": ["workers"],
                    "artifact": {
                        "path": "plugin.component.wasm",
                        "digest": digest(component.read_bytes()),
                        "size": 8,
                        "media_type": "application/wasm",
                        "target": "wasm32-unknown-unknown",
                    },
                    "runtime": {
                        "runtime_profile": "lenso.wasm-component@1",
                        "entrypoint": "plugin",
                        "required_target_capabilities": ["request", "wasm-component", "workers"],
                    },
                }],
            }
            manifest.write_text(json.dumps(document, indent=2) + "\n")
            pretty, pretty_digest = prepare(manifest, component, root / "pretty")
            canonical = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
            manifest.write_bytes(canonical)
            compact, compact_digest = prepare(manifest, component, root / "compact")
            self.assertEqual(pretty_digest, compact_digest)
            self.assertEqual(pretty.read_bytes(), compact.read_bytes())
            self.assertEqual(json.loads(pretty.read_bytes())["manifest_digest"], digest(canonical))

    def test_rejects_input_output_and_ancestor_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            manifest, component = self.inputs(root)
            link = root / "link"
            link.symlink_to(manifest)
            with self.assertRaises(ValueError):
                prepare(link, component, root / "output")
            with self.assertRaises(ValueError):
                prepare(manifest, component, link)
            ancestor = root / "ancestor"
            ancestor.symlink_to(root, target_is_directory=True)
            with self.assertRaises(ValueError):
                prepare(manifest, component, ancestor / "output")
            self.assertFalse((root / "output").exists())


if __name__ == "__main__":
    unittest.main()
