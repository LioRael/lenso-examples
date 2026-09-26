import io
import json
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch

from openapi_snapshot import (
    expected_runtime_document,
    select_runtime_document,
    verify_runtime_document,
)


SNAPSHOT = Path(__file__).parent / "project" / "frontend" / "openapi.json"


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


if __name__ == "__main__":
    unittest.main()
