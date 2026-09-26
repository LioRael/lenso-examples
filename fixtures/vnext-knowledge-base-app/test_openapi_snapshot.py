import ast
import io
import json
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from contextlib import contextmanager, redirect_stdout
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from openapi_snapshot import (
    expected_runtime_document,
    select_runtime_document,
    verify_runtime_document,
)


SNAPSHOT = Path(__file__).parent / "project" / "frontend" / "openapi.json"
VERIFY = Path(__file__).with_name("verify.py")


class Response(io.BytesIO):
    status = 200
    headers = {"Content-Type": "application/json"}


class OpenApiSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.snapshot = json.loads(SNAPSHOT.read_text())

    def test_selected_plugin_config_uses_exact_react_components(self):
        with tempfile.TemporaryDirectory() as temporary:
            project = Path(temporary)
            config = project / "plugins" / "lenso.openapi" / "default.toml"
            config.parent.mkdir(parents=True)
            config.write_text("# selected by app add\n")
            self.assertEqual(select_runtime_document(project, SNAPSHOT), self.snapshot)
            selected = tomllib.loads(config.read_text())
            self.assertEqual(selected["title"], self.snapshot["info"]["title"])
            self.assertEqual(selected["version"], self.snapshot["info"]["version"])
            self.assertEqual(selected["components"], self.snapshot["components"])

    def test_live_comparison_includes_schemas_and_only_known_static_routes(self):
        document = expected_runtime_document(self.snapshot)
        with patch("openapi_snapshot.urllib.request.urlopen", return_value=Response(json.dumps(document).encode())):
            self.assertEqual(verify_runtime_document("http://127.0.0.1:1234", self.snapshot), (7, 10))

        document = json.loads(json.dumps(document))
        document["components"]["schemas"]["Note"]["required"].append("unexpected")
        with patch("openapi_snapshot.urllib.request.urlopen", return_value=Response(json.dumps(document).encode())):
            with self.assertRaisesRegex(RuntimeError, "components"):
                verify_runtime_document("http://127.0.0.1:1234", self.snapshot)

    def test_live_comparison_rejects_extra_internal_route(self):
        document = expected_runtime_document(self.snapshot)
        document["paths"]["/_internal"] = {"get": {"operationId": "unexpected"}}
        with patch("openapi_snapshot.urllib.request.urlopen", return_value=Response(json.dumps(document).encode())):
            with self.assertRaisesRegex(RuntimeError, "paths"):
                verify_runtime_document("http://127.0.0.1:1234", self.snapshot)

    def test_browser_handoff_issues_token_before_source_deletion_and_starts_distribution(self):
        tree = ast.parse(VERIFY.read_text(encoding="utf-8"))
        branch = next(
            node for node in ast.walk(tree)
            if isinstance(node, ast.If)
            and isinstance(node.test, ast.Attribute)
            and isinstance(node.test.value, ast.Name)
            and node.test.value.id == "args"
            and node.test.attr == "openapi_only"
            and any(
                isinstance(child, ast.Call)
                and isinstance(child.func, ast.Attribute)
                and isinstance(child.func.value, ast.Name)
                and child.func.value.id == "shutil"
                and child.func.attr == "rmtree"
                for statement in node.body for child in ast.walk(statement)
            )
        )
        executable = compile(ast.Module(body=branch.body, type_ignores=[]), str(VERIFY), "exec")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            distribution = root / "dist"
            distribution.mkdir()
            events = []

            def issue_tokens():
                self.assertTrue(source.is_dir())
                events.append("issued")
                return {"user-a": "dummy-token"}

            def launch(actual_cli, actual_distribution, actual_root, environment):
                self.assertEqual((actual_cli, actual_distribution, actual_root),
                                 ("lenso", distribution, root))
                self.assertFalse(source.exists())
                events.append("launched")
                return object(), object(), object(), "http://127.0.0.1:1234"

            @contextmanager
            def handoff(path, token, url):
                self.assertEqual((path, token, url),
                                 (root / "handoff.json", "dummy-token", "http://127.0.0.1:1234"))
                events.append("handoff")
                yield SimpleNamespace(wait_for_removal=lambda: events.append("removed"))

            def compare(url, snapshot):
                self.assertEqual(url, "http://127.0.0.1:1234")
                self.assertEqual(snapshot, self.snapshot)
                events.append("compared")
                return 7, 10

            namespace = {
                "args": SimpleNamespace(browser_handoff=root / "handoff.json"),
                "issue_tokens": issue_tokens,
                "shutil": shutil,
                "source": source,
                "launch": launch,
                "cli": "lenso",
                "distribution": distribution,
                "root": root,
                "runtime_environment": {},
                "verify_runtime_document": compare,
                "openapi_snapshot": self.snapshot,
                "browser_handoff": handoff,
                "stop": lambda *_: events.append("stopped"),
            }
            with redirect_stdout(io.StringIO()) as output, self.assertRaises(SystemExit) as exit_status:
                exec(executable, namespace)
            self.assertEqual(exit_status.exception.code, 0)
            self.assertEqual(events, ["issued", "launched", "compared", "handoff", "removed", "stopped"])
            self.assertIn("PASS: live Host /openapi.json", output.getvalue())

    def test_package_only_rejects_openapi_source_mode_before_adoption(self):
        result = subprocess.run(
            [sys.executable, str(VERIFY), "--package-only", "--openapi-only"],
            env={"LENSO_REFERENCE_DATABASE_URL": "postgresql://invalid"},
            capture_output=True, text=True, check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--openapi-only requires the unpublished 0.2.5 source candidate",
                      result.stderr)


if __name__ == "__main__":
    unittest.main()
