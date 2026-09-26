"""Bind the React OpenAPI snapshot to the optional runtime document."""

import json
import tomllib
import urllib.request


JSON_SCHEMA_DIALECT = "https://json-schema.org/draft/2020-12/schema"
STATIC_ENDPOINTS = {
    "/": "knowledge-base.home",
    "/assets/app.js": "knowledge-base.assets.js",
    "/assets/index.css": "knowledge-base.assets.css",
}


def _toml_value(value):
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, list):
        return "[" + ", ".join(_toml_value(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{ " + ", ".join(
            f"{_toml_value(key)} = {_toml_value(item)}" for key, item in value.items()
        ) + " }"
    raise ValueError(f"unsupported OpenAPI config value: {type(value).__name__}")


def select_runtime_document(project, snapshot_path):
    """Configure the already-adopted OpenAPI Plugin from the client snapshot."""
    snapshot = json.loads(snapshot_path.read_text())
    if set(snapshot) != {"openapi", "info", "paths", "components"}:
        raise ValueError("React OpenAPI snapshot has unexpected top-level fields")
    if snapshot["openapi"] != "3.1.0":
        raise ValueError("React OpenAPI snapshot must use 3.1.0")
    info = snapshot["info"]
    if set(info) != {"title", "version"} or not all(isinstance(v, str) for v in info.values()):
        raise ValueError("React OpenAPI snapshot has unexpected info")
    if not isinstance(snapshot["components"], dict):
        raise ValueError("React OpenAPI snapshot must declare components")
    config = (
        f"title = {_toml_value(info['title'])}\n"
        f"version = {_toml_value(info['version'])}\n"
        f"components = {_toml_value(snapshot['components'])}\n"
    )
    if tomllib.loads(config)["components"] != snapshot["components"]:
        raise ValueError("OpenAPI components changed while encoding Plugin configuration")
    path = project / "plugins" / "lenso.openapi" / "default.toml"
    if not path.is_file():
        raise ValueError("OpenAPI Plugin must be selected before configuration")
    path.write_text(config)
    return snapshot


def expected_runtime_document(snapshot):
    expected = dict(snapshot)
    expected["jsonSchemaDialect"] = JSON_SCHEMA_DIALECT
    paths = dict(snapshot["paths"])
    for path, route_id in STATIC_ENDPOINTS.items():
        if path in paths:
            raise ValueError(f"static asset path entered the React client snapshot: {path}")
        paths[path] = {"get": {
            "operationId": route_id,
            "responses": {"default": {"description": "Undocumented response."}},
        }}
    expected["paths"] = paths
    return expected


def verify_runtime_document(url, snapshot):
    with urllib.request.urlopen(url.rstrip("/") + "/openapi.json", timeout=10) as response:
        if response.status != 200:
            raise RuntimeError(f"OpenAPI Endpoint returned HTTP {response.status}")
        if "application/json" not in response.headers.get("Content-Type", ""):
            raise RuntimeError("OpenAPI Endpoint did not return JSON")
        actual = json.load(response)
    expected = expected_runtime_document(snapshot)
    if actual != expected:
        for key in expected.keys() | actual.keys():
            if actual.get(key) != expected.get(key):
                raise RuntimeError(f"runtime OpenAPI drifted from React snapshot at {key}")
        raise RuntimeError("runtime OpenAPI drifted from React snapshot")
    return (
        sum(len(methods) for methods in snapshot["paths"].values()),
        sum(len(methods) for methods in actual["paths"].values()),
    )
