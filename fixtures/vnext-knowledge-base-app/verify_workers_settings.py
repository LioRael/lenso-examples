"""One HTTP corpus over Native and real local workerd backed by the same KB rows.

Run before-restart, restart only the task-owned workerd process/container, then
run after-restart while verify.py keeps the Native App and Auth tokens alive.
"""

import argparse
import base64
import concurrent.futures
import hashlib
import json
import os
import stat
import time
import uuid
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import urlsplit


MAX_JSON_BYTES = 65_536
DESCRIPTOR_DIGEST = "sha256:701deedf705cb1a3b2f35fcae72f20ae85d46c6da6a008405a519018bbcdd3fe"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, new_url):
        return None


def local_origin(value):
    parsed = urlsplit(value)
    if (parsed.scheme != "http" or parsed.hostname != "127.0.0.1"
            or parsed.port is None or parsed.path not in ("", "/")
            or parsed.query or parsed.fragment or parsed.username or parsed.password
            or parsed.netloc != f"127.0.0.1:{parsed.port}"):
        raise ValueError("acceptance endpoints must be explicit 127.0.0.1 HTTP origins")
    return f"http://127.0.0.1:{parsed.port}"


def private_json(path, *, max_bytes=MAX_JSON_BYTES):
    metadata = path.stat(follow_symlinks=False)
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o777 != 0o600:
        raise ValueError(f"expected a mode-0600 regular file: {path}")
    if metadata.st_size > max_bytes:
        raise ValueError(f"acceptance file exceeds its bound: {path}")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        opened = os.fstat(fd)
        if (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino):
            raise RuntimeError("acceptance file changed while opening")
        payload = os.read(fd, max_bytes + 1)
    finally:
        os.close(fd)
    if len(payload) > max_bytes:
        raise ValueError("acceptance file exceeds its bound")
    return json.loads(payload)


def write_private_json(path, value):
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    if len(payload) > MAX_JSON_BYTES:
        raise ValueError("acceptance state exceeds its bound")
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "wb", closefd=False) as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
    finally:
        os.close(fd)


def handoff(path):
    payload = private_json(path, max_bytes=4096)
    if set(payload) != {"token", "url"} or set(payload["token"]) != {"user-a", "user-b"}:
        raise ValueError("Native handoff must contain only two Auth-issued user tokens and URL")
    tokens = payload["token"]
    for token in tokens.values():
        if not isinstance(token, str) or not token.startswith("lenso_at_"):
            raise ValueError("Native Auth handoff is not an API-token credential")
        encoded = token[len("lenso_at_"):]
        if len(encoded) != 43:
            raise ValueError("Native Auth handoff has an unexpected API-token length")
        try:
            secret = base64.b64decode(encoded + "=", altchars=b"-_", validate=True)
        except ValueError as error:
            raise ValueError("Native Auth handoff has invalid API-token encoding") from error
        if len(secret) != 32:
            raise ValueError("Native Auth handoff has invalid API-token secret length")
    return local_origin(payload["url"]), tokens


def create_bridge_policy(path, tokens, ttl_seconds):
    if not 1 <= ttl_seconds <= 3600:
        raise ValueError("bridge policy TTL must be from 1 through 3600 seconds")
    if tokens["user-a"] == tokens["user-b"]:
        raise ValueError("two Native actors cannot share one credential")
    write_private_json(path, {
        "schema": "lenso.knowledge-settings-auth-policy.v1",
        "expires_unix": int(time.time()) + ttl_seconds,
        "actors": [
            {"owner_id": owner, "token_sha256": "sha256:" + hashlib.sha256(tokens[owner].encode()).hexdigest()}
            for owner in ("user-a", "user-b")
        ],
    })


def checked_workers_build(path):
    raw = path.read_bytes()
    if len(raw) > MAX_JSON_BYTES:
        raise ValueError("Workers build receipt exceeds its bound")
    receipt = json.loads(raw)
    if (receipt.get("schema") != "lenso.workers-app-build.v1"
            or receipt.get("environment") != "local-workerd"
            or receipt.get("plugin_id") != "lenso.reference.knowledge-settings"
            or receipt.get("private_world") != "lenso:knowledge-settings-local@1.0.0/plugin"
            or receipt.get("host_bridge") != "local-loopback-knowledge-settings.v1"
            or receipt.get("expected_descriptor_digests", {}).get("lenso.http.endpoint@1") != DESCRIPTOR_DIGEST
            or receipt.get("workers_runtime", {}).get("version") != "0.1.5"):
        raise ValueError("Workers build receipt is not the exact local knowledge-settings slice")
    for name, digest in {
        "bundles/lenso.reference.knowledge-settings.lenso-plugin": receipt.get("bundle_digest"),
        "guest.component.wasm": receipt.get("component_digest"),
        "guest.core.wasm": receipt.get("jco_core_digest"),
        **receipt["workers_runtime"].get("module_digests", {}),
    }.items():
        if (not isinstance(digest, str) or not digest.startswith("sha256:")
                or len(digest) != 71):
            raise ValueError(f"Workers build receipt lacks a valid digest for {name}")
        artifact = path.parent / name
        if not artifact.is_file() or "sha256:" + hashlib.sha256(artifact.read_bytes()).hexdigest() != digest:
            raise ValueError(f"Workers build artifact does not match its receipt: {name}")
    return "sha256:" + hashlib.sha256(raw).hexdigest()


def endpoint(origin, method, token=None, body=None, key=None, query=""):
    headers = {}
    encoded = None
    if token is not None:
        headers["Authorization"] = "Bearer " + token
    if key is not None:
        headers["Idempotency-Key"] = key
    if body is not None:
        encoded = json.dumps(body, separators=(",", ":")).encode()
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(
        origin + "/settings" + query, data=encoded, headers=headers, method=method
    )
    try:
        response = urllib.request.build_opener(NoRedirect).open(request, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        status = response.status
        raw = response.read(MAX_JSON_BYTES + 1)
    if len(raw) > MAX_JSON_BYTES:
        raise AssertionError("settings response exceeds the acceptance bound")
    try:
        value = json.loads(raw)
    except (ValueError, UnicodeDecodeError) as error:
        raise AssertionError(f"settings response is not JSON: HTTP {status}") from error
    return status, value


def malformed_body(origin, token):
    request = urllib.request.Request(
        origin + "/settings", data=b"{invalid", method="PUT",
        headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
    )
    try:
        response = urllib.request.build_opener(NoRedirect).open(request, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status


def expect(origin, method, status, token=None, body=None, key=None, query="", code=None):
    actual_status, value = endpoint(origin, method, token, body, key, query)
    if actual_status != status:
        raise AssertionError(f"{method} /settings: expected HTTP {status}, got {actual_status}")
    if code is not None and value.get("code") != code:
        raise AssertionError(f"{method} /settings: expected problem code {code!r}")
    return value


def settings(value):
    if (not isinstance(value, dict) or set(value) != {"excerpt_limit", "revision"}
            or type(value["excerpt_limit"]) is not int
            or type(value["revision"]) is not int
            or not 16 <= value["excerpt_limit"] <= 512 or value["revision"] < 1):
        raise AssertionError("settings response has an invalid business shape")
    return value


def before_restart(native, workers, tokens, build_digest, generation, state_path):
    a, b = tokens["user-a"], tokens["user-b"]
    initial_a = settings(expect(native, "GET", 200, a))
    initial_b = settings(expect(native, "GET", 200, b))
    assert settings(expect(workers, "GET", 200, a)) == initial_a
    assert settings(expect(workers, "GET", 200, b)) == initial_b
    for origin in (native, workers):
        assert settings(expect(origin, "GET", 200, a, query="?probe=1")) == initial_a

    first = {"excerpt_limit": 48, "predecessor_revision": initial_a["revision"]}
    first_key = "native-" + uuid.uuid4().hex
    native_written = settings(expect(native, "PUT", 200, a, first, first_key))
    assert native_written == {"excerpt_limit": 48, "revision": initial_a["revision"] + 1}
    assert settings(expect(native, "PUT", 200, a, first, first_key)) == native_written
    assert settings(expect(workers, "GET", 200, a)) == native_written

    second = {"excerpt_limit": 64, "predecessor_revision": native_written["revision"]}
    second_key = "workers-" + uuid.uuid4().hex
    workers_written = settings(expect(workers, "PUT", 200, a, second, second_key))
    assert workers_written == {"excerpt_limit": 64, "revision": native_written["revision"] + 1}
    assert settings(expect(native, "GET", 200, a)) == workers_written
    assert settings(expect(native, "PUT", 200, a, second, second_key)) == workers_written
    expect(native, "PUT", 409, a, {**second, "excerpt_limit": 80}, second_key,
           code="idempotency_conflict")
    expect(workers, "PUT", 409, a, {"excerpt_limit": 80, "predecessor_revision": initial_a["revision"]},
           code="stale_settings_revision")
    for origin in (native, workers):
        expect(origin, "PUT", 409, a, {"excerpt_limit": 80, "predecessor_revision": -1},
               code="stale_settings_revision")
        expect(origin, "PUT", 400, a, {"excerpt_limit": 15, "predecessor_revision": workers_written["revision"]},
               code="invalid_excerpt_limit")
        expect(origin, "PUT", 400, a, {"excerpt_limit": 513, "predecessor_revision": workers_written["revision"]},
               code="invalid_excerpt_limit")
        expect(origin, "PUT", 400, a, {"excerpt_limit": 80})
        assert malformed_body(origin, a) == 400
        expect(origin, "PUT", 400, a, {"excerpt_limit": 80, "predecessor_revision": workers_written["revision"]},
               "two,keys", code="invalid_idempotency_key")
        expect(origin, "GET", 401)
        expect(origin, "GET", 401, "invalid-" + uuid.uuid4().hex)
    assert settings(expect(workers, "GET", 200, b)) == initial_b

    user_b_update = {"excerpt_limit": 80, "predecessor_revision": initial_b["revision"]}
    final_b = settings(expect(native, "PUT", 200, b, user_b_update, second_key))
    assert final_b == {"excerpt_limit": 80, "revision": initial_b["revision"] + 1}
    assert settings(expect(workers, "GET", 200, b)) == final_b
    assert settings(expect(native, "GET", 200, a)) == workers_written

    concurrent_body = {"excerpt_limit": 128, "predecessor_revision": workers_written["revision"]}
    concurrent_key = "cross-target-" + uuid.uuid4().hex
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        first_future = pool.submit(expect, native, "PUT", 200, a, concurrent_body, concurrent_key)
        second_future = pool.submit(expect, workers, "PUT", 200, a, concurrent_body, concurrent_key)
        first_result = settings(first_future.result())
        second_result = settings(second_future.result())
    final_a = {"excerpt_limit": 128, "revision": workers_written["revision"] + 1}
    assert first_result == second_result == final_a
    assert settings(expect(native, "GET", 200, a)) == final_a
    assert settings(expect(workers, "GET", 200, a)) == final_a
    expect(workers, "PUT", 409, a, {**concurrent_body, "predecessor_revision": final_a["revision"]},
           concurrent_key, code="idempotency_conflict")

    write_private_json(state_path, {
        "schema": "lenso.knowledge-settings-cross-target.v1",
        "workers_build_digest": build_digest,
        "before_workerd_generation": generation,
        "expected_user_a": final_a,
        "expected_user_b": final_b,
        "replay_key": concurrent_key,
        "replay_body": concurrent_body,
    })
    return final_a, final_b


def after_restart(native, workers, tokens, build_digest, generation, state_path):
    state = private_json(state_path)
    if (state.get("schema") != "lenso.knowledge-settings-cross-target.v1"
            or state.get("workers_build_digest") != build_digest
            or state.get("before_workerd_generation") == generation):
        raise ValueError("restart phase requires the same verified build and a new workerd generation")
    a, b = tokens["user-a"], tokens["user-b"]
    for origin in (native, workers):
        assert settings(expect(origin, "GET", 200, a)) == state["expected_user_a"]
        assert settings(expect(origin, "GET", 200, b)) == state["expected_user_b"]
        assert settings(expect(origin, "PUT", 200, a, state["replay_body"], state["replay_key"])) == state["expected_user_a"]
        expect(origin, "PUT", 409, a, {**state["replay_body"], "excerpt_limit": 160},
               state["replay_key"], code="idempotency_conflict")
    return state["expected_user_a"], state["expected_user_b"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare-policy", "before-restart", "after-restart"))
    parser.add_argument("--handoff", type=Path, required=True,
                        help="mode-0600 verify.py --workers-settings-handoff JSON")
    parser.add_argument("--workers-url")
    parser.add_argument("--workers-build", type=Path)
    parser.add_argument("--state", type=Path,
                        help="mode-0600 no-token state file, created by before-restart")
    parser.add_argument("--workerd-generation",
                        help="observed task-owned workerd process/container start identity")
    parser.add_argument("--policy", type=Path, help="new mode-0600 Host-owned bridge policy path")
    parser.add_argument("--policy-ttl-seconds", type=int, default=1800)
    args = parser.parse_args()
    native, tokens = handoff(args.handoff)
    if args.phase == "prepare-policy":
        if args.policy is None:
            parser.error("prepare-policy requires --policy")
        create_bridge_policy(args.policy, tokens, args.policy_ttl_seconds)
        print(json.dumps({"phase": args.phase, "policy": str(args.policy)}))
        return
    if not all((args.workers_url, args.workers_build, args.state, args.workerd_generation)):
        parser.error("HTTP phases require --workers-url, --workers-build, --state, and --workerd-generation")
    workers = local_origin(args.workers_url)
    digest = checked_workers_build(args.workers_build)
    if args.phase == "before-restart":
        a, b = before_restart(native, workers, tokens, digest, args.workerd_generation, args.state)
    else:
        a, b = after_restart(native, workers, tokens, digest, args.workerd_generation, args.state)
    print(json.dumps({"phase": args.phase, "user_a_revision": a["revision"],
                      "user_b_revision": b["revision"], "workers_build_digest": digest}))


if __name__ == "__main__":
    main()
