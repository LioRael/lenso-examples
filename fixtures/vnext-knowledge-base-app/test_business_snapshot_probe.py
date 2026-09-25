import io
import json
import stat
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import patch

from business_snapshot_probe import (
    await_policy_status,
    expect_upload_problem,
    host_policy,
    persisted_revisions,
    postgres_client_environment,
    snapshot,
    write_private_json,
)


class BusinessSnapshotProbeTests(unittest.TestCase):
    def test_host_policy_owns_exact_object_and_file_source(self):
        source = Path("/private/tmp/attachment-source.json")
        policy = host_policy(source)
        self.assertEqual(policy["object"], snapshot(1, 32)["object"])
        self.assertEqual(policy["source"]["path"], str(source))
        self.assertEqual(policy["source"]["kind"], "file")
        self.assertNotIn("credential", json.dumps(policy))

    def test_snapshot_is_atomically_replaced_with_private_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "source.json"
            write_private_json(target, snapshot(1, 32))
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)
            write_private_json(target, snapshot(2, 64))
            self.assertEqual(json.loads(target.read_text())["revision"], 2)
            self.assertFalse(target.with_name("source.json.next").exists())

    def test_local_database_environment_keeps_password_out_of_url_argument(self):
        environment = postgres_client_environment(
            "postgresql://operator:secret%2Bvalue@127.0.0.1:5432/disposable"
        )
        self.assertEqual(environment["PGDATABASE"], "disposable")
        self.assertEqual(environment["PGPASSWORD"], "secret+value")
        with self.assertRaisesRegex(ValueError, "local PostgreSQL"):
            postgres_client_environment("postgresql://operator@example.com/prod")
        with patch.dict("os.environ", {"PGPASSWORD": "stale-credential"}):
            self.assertNotIn(
                "PGPASSWORD",
                postgres_client_environment("postgresql://operator@127.0.0.1/disposable"),
            )

    def test_persisted_rows_reject_untrusted_attachment_id_before_sql(self):
        with patch("business_snapshot_probe.subprocess.run") as run:
            with self.assertRaisesRegex(AssertionError, "invalid attachment identity"):
                persisted_revisions("postgresql://postgres@127.0.0.1/disposable", [{
                    "id": "attachment-x'); DROP TABLE knowledge_reference.attachments; --",
                    "policy_revision": "1", "size": 1,
                }])
            run.assert_not_called()

    def test_upload_problem_requires_exact_status_and_code(self):
        body = io.BytesIO(b'{"code":"attachment_policy_unavailable"}')
        error = urllib.error.HTTPError("http://127.0.0.1/upload", 503, "Unavailable", {}, body)
        with patch("urllib.request.urlopen", side_effect=error):
            expect_upload_problem("http://127.0.0.1:1", "opaque", "note-1", b"x",
                                  503, "attachment_policy_unavailable")

    def test_policy_transition_poll_uses_malformed_nonwriting_upload(self):
        def problem(status, code):
            return urllib.error.HTTPError(
                "http://127.0.0.1/upload", status, "Problem", {},
                io.BytesIO(json.dumps({"code": code}).encode()),
            )

        errors = [problem(400, "invalid_attachment"),
                  problem(503, "attachment_policy_unavailable")]

        def probe(request, timeout):
            self.assertIn(b'"content_base64":"!!!"', request.data)
            raise errors.pop(0)

        with patch("urllib.request.urlopen", side_effect=probe):
            await_policy_status("http://127.0.0.1:1", "opaque", "note-1",
                                (503, "attachment_policy_unavailable"), timeout=1)
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
