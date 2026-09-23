#!/usr/bin/env python3
"""Build and verify the source-deleted knowledge base reference application."""
import argparse
import base64
import json
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import tomllib
import urllib.error
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--cli", default="lenso")
parser.add_argument("--auth-source", required=True, help="local API Token Auth source crate")
parser.add_argument("--jobs-source", required=True, help="local Jobs source crate")
parser.add_argument("--secrets-source", required=True, help="local environment Secrets source crate")
parser.add_argument(
    "--browser-handoff",
    type=Path,
    help="optional temporary JSON handoff; delete it after browser acceptance to continue",
)
parser.add_argument(
    "--web-client-package",
    help="optional @lenso/web-client .tgz used to rebuild and typecheck the React UI",
)
args = parser.parse_args()
cli = str(Path(shutil.which(args.cli) or args.cli).absolute())
fixture = Path(__file__).resolve().parent
database_url = os.environ.get("LENSO_REFERENCE_DATABASE_URL")
if not database_url:
    parser.error("LENSO_REFERENCE_DATABASE_URL must name a disposable PostgreSQL database")


def repository_for(crate, expected_plugin_id):
    crate = Path(crate).resolve()
    if not (crate / "Cargo.toml").is_file() or crate.parent.name != "crates":
        parser.error(f"source crate must be a crates/<package> directory: {crate}")
    with (crate / "Cargo.toml").open("rb") as manifest:
        package = tomllib.load(manifest).get("package", {})
    actual_plugin_id = package.get("metadata", {}).get("lenso", {}).get("plugin-id")
    if actual_plugin_id != expected_plugin_id:
        parser.error(
            f"{crate} must declare package.metadata.lenso.plugin-id = "
            f"{expected_plugin_id!r}; found {actual_plugin_id!r}. "
            "Select a source revision that supports linked App adoption."
        )
    return crate.parents[1]


repositories = {
    "auth": repository_for(args.auth_source, "lenso.auth.api-token"),
    "jobs": repository_for(args.jobs_source, "lenso.jobs"),
    "secrets": repository_for(args.secrets_source, "lenso.secrets.env"),
}


def run(command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def operator_environment(**values):
    return os.environ | values


def launch(cli, distribution, root, environment, app_root=None):
    command = [cli, "app", "start", "--from", str(distribution)]
    if app_root is not None:
        command.extend(["--root", str(app_root)])
    process = subprocess.Popen(
        command,
        env=environment,
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    events = queue.Queue()
    transcript = []

    def read_output():
        for line in process.stdout:
            transcript.append(line)
            events.put(line)
        events.put(None)

    reader = threading.Thread(target=read_output, daemon=True)
    reader.start()
    try:
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            line = events.get(timeout=max(0.1, deadline - time.monotonic()))
            if line is None:
                raise RuntimeError("Host exited before readiness")
            match = re.search(r"Listening on (http://\S+)", line)
            if match:
                return process, reader, transcript, match[1]
        raise RuntimeError("Host did not report Web readiness")
    except BaseException:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        reader.join(timeout=2)
        print("".join(transcript))
        raise


def stop(process, reader, transcript):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        raise
    reader.join(timeout=2)
    print("".join(transcript))
    assert process.returncode == 0


def http_json(url, method="GET", body=None, token=None, expected=200):
    headers = {}
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status == expected
        return json.load(response)


def expect_http_error(url, code, method="GET", body=None, token=None):
    try:
        http_json(url, method=method, body=body, token=token)
    except urllib.error.HTTPError as error:
        assert error.code == code, error.read().decode()
    else:
        raise AssertionError(f"request must return HTTP {code}: {method} {url}")


with tempfile.TemporaryDirectory(prefix="lenso-knowledge-base-") as temporary:
    root = Path(temporary)
    source = root / "source"
    shutil.copytree(
        fixture,
        source,
        ignore=shutil.ignore_patterns(
            "target", ".lenso", "dist", "node_modules", "vendor", "generated", "__pycache__"
        ),
    )
    if args.web_client_package:
        frontend = source / "frontend"
        vendor = frontend / "vendor"
        vendor.mkdir()
        shutil.copyfile(Path(args.web_client_package).resolve(), vendor / "lenso-web-client.tgz")
        run(["bun", "install", "--frozen-lockfile"], cwd=frontend)
        run(["bun", "run", "generate"], cwd=frontend)
        run(["bun", "run", "typecheck"], cwd=frontend)
        run(["bun", "run", "build"], cwd=frontend)

    candidates = source / "candidates"
    for name, repository in repositories.items():
        shutil.copytree(
            repository,
            candidates / name,
            ignore=shutil.ignore_patterns(".git", ".worktrees", "target"),
        )
    auth_source = candidates / "auth" / "crates" / "lenso-auth-api-token-plugin"
    jobs_source = candidates / "jobs" / "crates" / "lenso-jobs-plugin"
    secrets_source = candidates / "secrets" / "crates" / "lenso-secrets-env-plugin"
    project = source / "project"
    for plugin_source in [auth_source, jobs_source, secrets_source]:
        run([cli, "app", "add", "--root", str(project), "--no-install", str(plugin_source)])

    suffix = f"{os.getpid()}_{secrets.randbelow(1_000_000)}"
    auth_schema = f"auth_reference_{suffix}"
    jobs_schema = f"jobs_reference_{suffix}"
    signing_secret = secrets.token_urlsafe(48)
    token_pepper = secrets.token_urlsafe(48)
    auth_operator = [
        "cargo", "run", "--locked", "-p", "lenso-auth-api-token-plugin",
        "--example", "api-token-operator", "--",
    ]
    auth_environment = operator_environment(
        LENSO_AUTH_DATABASE_URL=database_url,
        LENSO_AUTH_SIGNING_SECRET=signing_secret,
        LENSO_AUTH_TOKEN_PEPPER=token_pepper,
    )
    run(auth_operator + ["setup", auth_schema], cwd=candidates / "auth", env=auth_environment)
    public_key = subprocess.run(
        auth_operator + ["public-key"],
        cwd=candidates / "auth",
        env=auth_environment,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    tokens = {}
    for subject in ["user-a", "user-b"]:
        output = subprocess.run(
            auth_operator
            + ["issue", auth_schema, subject, "lenso.reference.knowledge-base@1:access"],
            cwd=candidates / "auth",
            env=auth_environment,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        tokens[subject] = json.loads(output)["token"]

    run(
        [
            "cargo", "run", "--locked", "-p", "lenso-jobs-plugin",
            "--example", "jobs-operator", "--", "setup", jobs_schema,
        ],
        cwd=candidates / "jobs",
        env=operator_environment(LENSO_JOBS_DATABASE_URL=database_url),
    )
    notes_web = project / "app" / "notes-web"
    run(
        ["cargo", "run", "--locked", "--manifest-path", str(notes_web / "Cargo.toml"),
         "--example", "knowledge-operator", "--", "setup"],
        env=operator_environment(LENSO_KNOWLEDGE_DATABASE_URL=database_url),
    )

    (project / "plugins" / "lenso.auth.api-token" / "default.toml").write_text(
        f'''schema = "{auth_schema}"
issuer = "knowledge-reference"
assertion_public_key = "{public_key}"
database_url_secret = "auth/database-url"
assertion_signing_key_secret = "auth/signing-secret"
token_pepper_secret = "auth/token-pepper"
assertion_ttl_seconds = 300
'''
    )
    (project / "plugins" / "lenso.jobs" / "default.toml").write_text(
        f'''schema = "{jobs_schema}"
database_url_secret = "jobs/database-url"
lease_seconds = 30
retry_base_seconds = 5
retry_max_seconds = 300
queues = ["knowledge"]
producer_instances = ["lenso.reference.knowledge-excerpt/default"]
worker_instances = ["lenso.reference.knowledge-excerpt/default"]
observer_instances = ["lenso.reference.knowledge-excerpt/default"]
'''
    )
    (project / "plugins" / "lenso.secrets.env" / "default.toml").write_text(
        '''[references]
"auth/database-url" = "LENSO_REFERENCE_DATABASE_URL"
"auth/signing-secret" = "LENSO_AUTH_SIGNING_SECRET"
"auth/token-pepper" = "LENSO_AUTH_TOKEN_PEPPER"
"jobs/database-url" = "LENSO_REFERENCE_DATABASE_URL"
"knowledge/database-url" = "LENSO_REFERENCE_DATABASE_URL"
'''
    )

    excerpt = project / "app" / "excerpt"
    run(["bun", "install", "--frozen-lockfile"], cwd=excerpt)
    run(["bun", "run", "check"], cwd=excerpt)
    distribution = root / "dist"
    run([cli, "app", "build", "--root", str(project), "--out", str(distribution)])
    lifecycle_root = root / "runtime-app"
    (lifecycle_root / ".lenso").mkdir(parents=True)
    host_authority = lifecycle_root / ".lenso" / "host-build.json"
    shutil.copyfile(
        distribution / ".lenso" / "host-build.json",
        host_authority,
    )
    shutil.copytree(project / "plugins", lifecycle_root / "plugins")
    jobs_configuration = lifecycle_root / "plugins" / "lenso.jobs" / "default.toml"
    jobs_configuration_text = jobs_configuration.read_text()
    run([
        cli, "plugins", "bind", "--root", str(lifecycle_root),
        "lenso.reference.knowledge-excerpt", "jobs", "--absent",
    ])
    run([cli, "plugins", "disable", "--root", str(lifecycle_root), "lenso.jobs", "default"])
    assert jobs_configuration.read_text() == jobs_configuration_text
    host_authority.unlink()
    shutil.rmtree(source)
    runtime_environment = {
        "PATH": str(root / "no-tools"),
        "LENSO_REFERENCE_DATABASE_URL": database_url,
        "LENSO_AUTH_SIGNING_SECRET": signing_secret,
        "LENSO_AUTH_TOKEN_PEPPER": token_pepper,
    }
    process, reader, transcript, url = launch(cli, distribution, root, runtime_environment)
    created = None
    try:
        with urllib.request.urlopen(url, timeout=10) as response:
            home = response.read().decode()
            assert response.status == 200
            assert '<div id="root"></div>' in home
        with urllib.request.urlopen(url.rstrip("/") + "/assets/app.js", timeout=10) as response:
            assert response.status == 200
            assert "Knowledge base" in response.read().decode()

        expect_http_error(
            url.rstrip("/") + "/notes", 401, method="POST",
            body={"title": "Denied", "body": "No credential"},
        )
        settings = http_json(url.rstrip("/") + "/settings", token=tokens["user-a"])
        assert settings == {"excerpt_limit": 96, "revision": 1}
        settings = http_json(
            url.rstrip("/") + "/settings", method="PUT", token=tokens["user-a"],
            body={"excerpt_limit": 48, "predecessor_revision": 1},
        )
        assert settings == {"excerpt_limit": 48, "revision": 2}
        expect_http_error(
            url.rstrip("/") + "/settings", 409, method="PUT", token=tokens["user-a"],
            body={"excerpt_limit": 64, "predecessor_revision": 1},
        )

        note_body = (
            "Created from the offline distribution through a real TypeScript Plugin "
            "without a model credential or source checkout at runtime."
        )
        created = http_json(
            url.rstrip("/") + "/notes", method="POST", expected=201, token=tokens["user-a"],
            body={"title": "First note", "body": note_body},
        )
        assert created["id"].startswith("note-")
        assert created["excerpt"] == ""
        assert created["job_id"].startswith("job_")
        assert created["processing_status"] == "queued"
        processed = http_json(
            url.rstrip("/") + "/jobs/process-next", method="POST", body={}, token=tokens["user-a"]
        )
        assert processed == {"processed": True}
        created = http_json(
            url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]
        )
        assert created["excerpt"] == note_body[:47] + "…"
        assert created["processing_status"] == "succeeded"
        durable = http_json(
            url.rstrip("/") + "/job-status/" + created["job_id"], token=tokens["user-a"]
        )
        assert durable == {"attempts": 1, "jobId": created["job_id"], "status": "succeeded"}

        attachment = http_json(
            url.rstrip("/") + f"/note-attachments/{created['id']}",
            method="POST", expected=201, token=tokens["user-a"],
            body={
                "content_base64": base64.b64encode(b"reference attachment").decode(),
                "filename": "reference.txt",
                "media_type": "text/plain",
            },
        )
        assert attachment["note_id"] == created["id"]
        assert attachment["size"] == len(b"reference attachment")
        expect_http_error(
            url.rstrip("/") + f"/note-attachments/{created['id']}", 404,
            method="POST", token=tokens["user-b"],
            body={
                "content_base64": base64.b64encode(b"not my note").decode(),
                "filename": "denied.txt",
                "media_type": "text/plain",
            },
        )
        expect_http_error(
            url.rstrip("/") + f"/note-attachments/{created['id']}", 400,
            method="POST", token=tokens["user-a"],
            body={"content_base64": "!!!", "filename": "invalid.txt", "media_type": "text/plain"},
        )
        expect_http_error(
            url.rstrip("/") + "/notes/" + created["id"], 404, token=tokens["user-b"]
        )
        expect_http_error(
            url.rstrip("/") + "/job-status/" + created["job_id"], 404, token=tokens["user-b"]
        )
        if args.browser_handoff:
            handoff = args.browser_handoff.resolve()
            handoff.write_text(json.dumps({"token": tokens["user-a"], "url": url}))
            handoff.chmod(0o600)
            deadline = time.monotonic() + 600
            while handoff.exists() and time.monotonic() < deadline:
                time.sleep(0.25)
            if handoff.exists():
                raise TimeoutError(f"browser handoff was not removed: {handoff}")
    finally:
        stop(process, reader, transcript)

    process, reader, transcript, url = launch(
        cli, distribution, root, runtime_environment, app_root=lifecycle_root
    )
    try:
        preserved = http_json(
            url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]
        )
        assert preserved == created
        inline_body = "Jobs is disabled, so deterministic processing completes inline."
        inline = http_json(
            url.rstrip("/") + "/notes", method="POST", expected=201, token=tokens["user-a"],
            body={"title": "Disabled Jobs", "body": inline_body},
        )
        assert inline["excerpt"] == inline_body[:47] + "…"
        assert inline["job_id"] == "inline:" + inline["id"]
        assert inline["processing_status"] == "succeeded"
    finally:
        stop(process, reader, transcript)

    shutil.copyfile(distribution / ".lenso" / "host-build.json", host_authority)
    run([cli, "plugins", "enable", "--root", str(lifecycle_root), "lenso.jobs", "default"])
    run([
        cli, "plugins", "bind", "--root", str(lifecycle_root),
        "lenso.reference.knowledge-excerpt", "jobs", "lenso.jobs",
    ])
    assert jobs_configuration.read_text() == jobs_configuration_text
    host_authority.unlink()
    process, reader, transcript, url = launch(
        cli, distribution, root, runtime_environment, app_root=lifecycle_root
    )
    try:
        restarted = http_json(
            url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]
        )
        assert restarted == created
        durable = http_json(
            url.rstrip("/") + "/job-status/" + created["job_id"], token=tokens["user-a"]
        )
        assert durable["status"] == "succeeded"
        assert durable["attempts"] == 1
    finally:
        stop(process, reader, transcript)

    # Remove the root-supplied Jobs Plugin, not merely its disabled marker.
    # The CLI validates the candidate App before moving the Plugin Root to
    # recoverable trash. The immutable Host distribution and business data stay.
    shutil.copyfile(distribution / ".lenso" / "host-build.json", host_authority)
    run([
        cli, "plugins", "bind", "--root", str(lifecycle_root),
        "lenso.reference.knowledge-excerpt", "jobs", "--absent",
    ])
    run([cli, "plugins", "remove", "--root", str(lifecycle_root), "lenso.jobs"])
    assert not (lifecycle_root / "plugins" / "lenso.jobs").exists()
    removed_roots = list((lifecycle_root / ".lenso" / "trash").glob("lenso.jobs-*"))
    assert len(removed_roots) == 1
    assert (removed_roots[0] / "default.toml").read_text() == jobs_configuration_text
    run([cli, "app", "check", "--root", str(lifecycle_root)])
    shown = json.loads(subprocess.run(
        [cli, "app", "show", "--root", str(lifecycle_root), "--json"],
        check=True, capture_output=True, text=True,
    ).stdout)
    assert shown["kind"] == "lenso.app-show"
    assert all(not instance["id"].startswith("lenso.jobs/") for instance in shown["instances"])
    host_authority.unlink()
    process, reader, transcript, url = launch(
        cli, distribution, root, runtime_environment, app_root=lifecycle_root
    )
    try:
        assert http_json(url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]) == created
        assert http_json(url.rstrip("/") + "/settings", token=tokens["user-a"]) == settings
        expect_http_error(url.rstrip("/") + "/notes/" + created["id"], 404, token=tokens["user-b"])
        removed_jobs_body = "The persisted workspace survives removal of its optional Jobs Plugin."
        without_jobs = http_json(
            url.rstrip("/") + "/notes", method="POST", expected=201, token=tokens["user-a"],
            body={"title": "Removed Jobs", "body": removed_jobs_body},
        )
        assert without_jobs["job_id"] == "inline:" + without_jobs["id"]
        assert without_jobs["processing_status"] == "succeeded"
        assert without_jobs["excerpt"] == removed_jobs_body[:47] + "…"
    finally:
        stop(process, reader, transcript)

print(
    "PASS: source-deleted React, Auth isolation, PostgreSQL notes/files/settings, "
    "Rust-to-TypeScript durable Jobs, disable/enable/remove preservation, and restart"
)
