#!/usr/bin/env python3
"""Build the knowledge base App and prove its offline HTTP create/read path."""
import argparse
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--cli", default="lenso")
parser.add_argument(
    "--web-client-package",
    help="optional @lenso/web-client .tgz used to rebuild and typecheck the React UI",
)
parser.add_argument("--jobs-source", help="local lenso-jobs-plugin source crate")
parser.add_argument("--secrets-source", help="local lenso-secrets-env-plugin source crate")
args = parser.parse_args()
cli = str(Path(shutil.which(args.cli) or args.cli).absolute())
fixture = Path(__file__).resolve().parent
jobs_mode = args.jobs_source is not None or args.secrets_source is not None
if jobs_mode and not (args.jobs_source and args.secrets_source):
    parser.error("--jobs-source and --secrets-source must be provided together")
database_url = os.environ.get("LENSO_JOBS_DATABASE_URL")
if jobs_mode and not database_url:
    parser.error("LENSO_JOBS_DATABASE_URL is required with Jobs sources")


def launch(cli, distribution, root, environment):
    process = subprocess.Popen(
        [cli, "app", "start", "--from", str(distribution)],
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
        shutil.copyfile(
            Path(args.web_client_package).resolve(), vendor / "lenso-web-client.tgz"
        )
        subprocess.run(["bun", "install", "--frozen-lockfile"], cwd=frontend, check=True)
        subprocess.run(["bun", "run", "generate"], cwd=frontend, check=True)
        subprocess.run(["bun", "run", "typecheck"], cwd=frontend, check=True)
        subprocess.run(["bun", "run", "build"], cwd=frontend, check=True)
    if jobs_mode:
        candidates = source / "candidates"
        jobs_repository = Path(args.jobs_source).resolve().parents[1]
        secrets_repository = Path(args.secrets_source).resolve().parents[1]
        shutil.copytree(
            jobs_repository,
            candidates / "jobs",
            ignore=shutil.ignore_patterns(".git", ".worktrees", "target"),
        )
        shutil.copytree(
            secrets_repository,
            candidates / "secrets",
            ignore=shutil.ignore_patterns(".git", ".worktrees", "target"),
        )
        jobs_source = candidates / "jobs" / "crates" / "lenso-jobs-plugin"
        secrets_source = candidates / "secrets" / "crates" / "lenso-secrets-env-plugin"
        project = source / "project"
        subprocess.run(
            [cli, "app", "add", "--root", str(project), "--no-install", str(jobs_source)],
            check=True,
        )
        subprocess.run(
            [cli, "app", "add", "--root", str(project), "--no-install", str(secrets_source)],
            check=True,
        )
        schema_name = f"jobs_knowledge_reference_{os.getpid()}"
        (project / "plugins" / "lenso.jobs" / "default.toml").write_text(
            f'''schema = "{schema_name}"
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
            '[references]\n"jobs/database-url" = "LENSO_JOBS_DATABASE_URL"\n'
        )
        subprocess.run(
            [
                "cargo",
                "run",
                "--locked",
                "-p",
                "lenso-jobs-plugin",
                "--example",
                "jobs-operator",
                "--",
                "setup",
                schema_name,
            ],
            cwd=candidates / "jobs",
            check=True,
        )
    excerpt = source / "project" / "app" / "excerpt"
    subprocess.run(["bun", "install", "--frozen-lockfile"], cwd=excerpt, check=True)
    subprocess.run(["bun", "run", "check"], cwd=excerpt, check=True)
    distribution = root / "dist"
    subprocess.run(
        [cli, "app", "build", "--root", str(source / "project"), "--out", str(distribution)],
        check=True,
    )
    shutil.rmtree(source)
    runtime_environment = {"PATH": str(root / "no-tools")}
    if jobs_mode:
        runtime_environment["LENSO_JOBS_DATABASE_URL"] = database_url
    process, reader, transcript, url = launch(cli, distribution, root, runtime_environment)
    created = None
    try:
        with urllib.request.urlopen(url, timeout=10) as response:
            home = response.read().decode()
            assert response.status == 200
            assert '<div id="root"></div>' in home

        with urllib.request.urlopen(url.rstrip("/") + "/assets/app.js", timeout=10) as response:
            javascript = response.read().decode()
            assert response.status == 200
            assert response.headers.get_content_type() == "text/javascript"
            assert "Knowledge base" in javascript

        with urllib.request.urlopen(url.rstrip("/") + "/assets/index.css", timeout=10) as response:
            assert response.status == 200
            assert response.headers.get_content_type() == "text/css"

        note_body = (
            "Created from the offline distribution through a real TypeScript Plugin "
            "without a model credential or source checkout at runtime."
        )
        request = urllib.request.Request(
            url.rstrip("/") + "/notes",
            data=json.dumps({"title": "First note", "body": note_body}).encode(),
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            assert response.status == 201
            created = json.load(response)
        expected = {
            "id": "note-1",
            "title": "First note",
            "body": note_body,
        }
        if jobs_mode:
            assert {key: created[key] for key in expected} == expected
            assert created["excerpt"] == ""
            assert created["job_id"].startswith("job_")
            assert created["processing_status"] == "queued"
            process_request = urllib.request.Request(
                url.rstrip("/") + "/jobs/process-next", data=b"", method="POST"
            )
            with urllib.request.urlopen(process_request, timeout=10) as response:
                assert response.status == 200
                created = json.load(response)
            assert created["excerpt"] == note_body[:95] + "…"
            assert created["processing_status"] == "succeeded"
            with urllib.request.urlopen(
                url.rstrip("/") + "/job-status/" + created["job_id"], timeout=10
            ) as response:
                durable = json.load(response)
            assert durable == {
                "attempts": 1,
                "jobId": created["job_id"],
                "status": "succeeded",
            }
        else:
            assert created == expected | {
                "excerpt": note_body[:95] + "…",
                "job_id": "inline:note-1",
                "processing_status": "succeeded",
            }

        with urllib.request.urlopen(
            url.rstrip("/") + "/notes/" + created["id"], timeout=10
        ) as response:
            assert response.status == 200
            assert json.load(response) == created

        try:
            urllib.request.urlopen(url.rstrip("/") + "/notes/missing", timeout=10)
        except urllib.error.HTTPError as error:
            assert error.code == 404
        else:
            raise AssertionError("missing note must return HTTP 404")
    finally:
        stop(process, reader, transcript)

    if jobs_mode:
        process, reader, transcript, url = launch(cli, distribution, root, runtime_environment)
        try:
            with urllib.request.urlopen(
                url.rstrip("/") + "/job-status/" + created["job_id"], timeout=10
            ) as response:
                durable = json.load(response)
            assert durable["status"] == "succeeded"
            assert durable["attempts"] == 1
        finally:
            stop(process, reader, transcript)

suffix = ", durable linked Jobs and restart" if jobs_mode else ""
print(f"PASS: offline React, Rust-to-TypeScript excerpt, create/read, rejection, shutdown{suffix}")
