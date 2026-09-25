"""Optional live Host-owned attachment policy check for the disposable KB App."""

import base64
import json
import os
import re
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


OBJECT = {
    "plugin_id": "lenso.reference.knowledge-base",
    "instance_key": "default",
    "object_key": "attachment-policy",
}
ATTACHMENT_ID = re.compile(r"attachment-[0-9a-f-]{36}\Z")


def write_private_json(path, value):
    path = Path(path)
    temporary = path.with_name(path.name + ".next")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, separators=(",", ":"))
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def snapshot(revision, max_attachment_bytes):
    return {
        "schema": "lenso.business-snapshot.v1",
        "revision": revision,
        "object": OBJECT,
        "value": {"max_attachment_bytes": max_attachment_bytes},
    }


def host_policy(source_path):
    return {
        "schema": "lenso.host-business-snapshot-policy.v1",
        "object": OBJECT,
        "source": {
            "kind": "file",
            "reference": "qualification-attachment-policy",
            "path": str(source_path),
        },
        "poll_interval_millis": 100,
        "max_stale_millis": 600,
    }


def postgres_client_environment(database_url):
    parsed = urllib.parse.urlsplit(database_url)
    if parsed.scheme not in ("postgres", "postgresql") or parsed.hostname not in (
        "127.0.0.1", "::1", "localhost"
    ):
        raise ValueError("business snapshot probe requires a local PostgreSQL URL")
    database = urllib.parse.unquote(parsed.path.lstrip("/"))
    if not database or "/" in database:
        raise ValueError("business snapshot probe requires one disposable database")
    environment = os.environ.copy()
    environment.update({
        "PGHOST": parsed.hostname,
        "PGPORT": str(parsed.port or 5432),
        "PGDATABASE": database,
        "PGUSER": urllib.parse.unquote(parsed.username or "postgres"),
    })
    environment.pop("PGPASSWORD", None)
    if parsed.password is not None:
        environment["PGPASSWORD"] = urllib.parse.unquote(parsed.password)
    return environment


def persisted_revisions(database_url, attachments):
    if not all(ATTACHMENT_ID.fullmatch(item["id"]) for item in attachments):
        raise AssertionError("Host returned an invalid attachment identity")
    identities = ",".join("'" + item["id"] + "'" for item in attachments)
    query = (
        "SELECT attachment_id, policy_revision, octet_length(content) "
        "FROM knowledge_reference.attachments "
        f"WHERE attachment_id IN ({identities}) ORDER BY attachment_id"
    )
    result = subprocess.run(
        ["psql", "--no-psqlrc", "--tuples-only", "--no-align", "--field-separator=|", "-c", query],
        env=postgres_client_environment(database_url),
        check=True, capture_output=True, text=True,
    )
    rows = {}
    for line in result.stdout.splitlines():
        attachment_id, revision, size = line.split("|", 2)
        rows[attachment_id] = (revision, int(size))
    expected = {item["id"]: (item["policy_revision"], item["size"]) for item in attachments}
    if rows != expected:
        raise AssertionError("persisted attachment policy revision differs from HTTP")
    return rows


def expect_upload_problem(url, token, note_id, content, expected_status, expected_code):
    request = urllib.request.Request(
        url.rstrip("/") + f"/note-attachments/{note_id}",
        data=json.dumps({
            "content_base64": base64.b64encode(content).decode(),
            "filename": "policy-rejected.txt",
            "media_type": "text/plain",
        }).encode(),
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        urllib.request.urlopen(request, timeout=10)
    except urllib.error.HTTPError as error:
        if error.code != expected_status:
            raise AssertionError(f"upload returned HTTP {error.code}, expected {expected_status}")
        problem = json.load(error)
        if problem.get("code") != expected_code:
            raise AssertionError("upload returned the wrong bounded Problem code")
    else:
        raise AssertionError(f"upload unexpectedly succeeded; expected HTTP {expected_status}")


def probe_policy_status(url, token, note_id):
    request = urllib.request.Request(
        url.rstrip("/") + f"/note-attachments/{note_id}",
        data=b'{"content_base64":"!!!","filename":"probe.txt","media_type":"text/plain"}',
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        urllib.request.urlopen(request, timeout=10)
    except urllib.error.HTTPError as error:
        problem = json.load(error)
        result = error.code, problem.get("code")
        if result in ((400, "invalid_attachment"), (503, "attachment_policy_unavailable")):
            return result
        raise AssertionError("unexpected upload status while probing attachment policy")
    raise AssertionError("malformed upload unexpectedly succeeded")


def await_policy_status(url, token, note_id, expected, timeout=4):
    deadline = time.monotonic() + timeout
    while True:
        status = probe_policy_status(url, token, note_id)
        if status == expected:
            return
        if time.monotonic() >= deadline:
            raise AssertionError(f"Host did not reach expected attachment policy status {expected}")
        time.sleep(0.1)


def verify_live_attachment_policy(*, cli, distribution, root, environment, database_url,
                                  note_id, token, expected_settings, launch, stop, http_json):
    directory = root / "host-business-snapshot"
    directory.mkdir(mode=0o700)
    source = directory / "attachment-source.json"
    policy = directory / "host-policy.json"
    write_private_json(source, snapshot(1, 32))
    write_private_json(policy, host_policy(source))
    process, reader, transcript, url = launch(
        cli, distribution, root, environment, business_snapshot_policy=policy,
    )
    accepted = []

    def upload(content, revision, prior_errors=()):
        request = urllib.request.Request(
            url.rstrip("/") + f"/note-attachments/{note_id}",
            data=json.dumps({
                "content_base64": base64.b64encode(content).decode(),
                "filename": f"policy-revision-{revision}.txt",
                "media_type": "text/plain",
            }).encode(),
            headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
            method="POST",
        )
        deadline = time.monotonic() + 4
        while True:
            try:
                with urllib.request.urlopen(request, timeout=10) as response:
                    if response.status != 201:
                        raise AssertionError("upload returned unexpected success status")
                    result = json.load(response)
                break
            except urllib.error.HTTPError as error:
                status = error.code, json.load(error).get("code")
                if status not in prior_errors:
                    raise AssertionError(f"upload failed with unexpected bounded status {status}")
                if time.monotonic() >= deadline:
                    raise AssertionError("Host did not observe the expected attachment revision")
                time.sleep(0.1)
        if result["policy_revision"] != str(revision) or result["size"] != len(content):
            raise AssertionError("upload did not pin the authorized policy revision")
        accepted.append(result)

    try:
        await_policy_status(url, token, note_id, (400, "invalid_attachment"))
        if http_json(url.rstrip("/") + "/settings", token=token) != expected_settings:
            raise AssertionError("Host policy changed per-user PostgreSQL settings")
        upload(b"revision-one", 1)
        expect_upload_problem(url, token, note_id, b"x" * 48, 400, "invalid_attachment")

        write_private_json(source, snapshot(2, 64))
        upload(b"x" * 48, 2, ((400, "invalid_attachment"),
                                 (503, "attachment_policy_unavailable")))

        unavailable = directory / "attachment-source.unavailable"
        source.rename(unavailable)
        await_policy_status(url, token, note_id, (503, "attachment_policy_unavailable"))
        if http_json(url.rstrip("/") + "/settings", token=token) != expected_settings:
            raise AssertionError("unavailable attachment source changed PostgreSQL settings")
        write_private_json(source, snapshot(2, 64))
        await_policy_status(url, token, note_id, (400, "invalid_attachment"))

        write_private_json(source, snapshot(2, 128))
        await_policy_status(url, token, note_id, (503, "attachment_policy_unavailable"))

        write_private_json(source, snapshot(3, 8))
        upload(b"recovery", 3, ((503, "attachment_policy_unavailable"),))
        expect_upload_problem(url, token, note_id, b"x" * 16, 400, "invalid_attachment")
    finally:
        stop(process, reader, transcript)

    rows = persisted_revisions(database_url, accepted)
    return {
        "source_kind": "Host-owned file",
        "selected_object": OBJECT,
        "accepted_revisions": [1, 2, 3],
        "same_revision_conflict_and_source_loss_failed_closed": True,
        "settings_unchanged": True,
        "persisted_attachment_rows": len(rows),
        "attachment_ids": [item["id"] for item in accepted],
    }
