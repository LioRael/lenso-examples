import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import client

class DispatchFenceTests(unittest.TestCase):
    def fixture(self, directory):
        root = Path(directory)
        os.chmod(root, 0o700)
        client.write_new(root / "cap.secret", "fixture-operator-capability-value-at-least-32")
        client.write_new(root / "operator-profile.json", {"url": "http://127.0.0.1:9000/_qualification/owners", "capability_file": "cap.secret"})
        return root

    def test_unknown_issuance_survives_restart_without_dispatching_again(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture(directory)
            action = {"action_id": "issue-a", "owner": "auth", "operation": "issue", "parameters": {}, "secret_output": "token.secret"}
            reply = type("Reply", (), {"returncode": 1, "stdout": "{}"})()
            with patch("client.subprocess.run", return_value=reply) as transport:
                self.assertEqual(client.execute(root, action)["state"], "unknown")
                with self.assertRaises(FileExistsError):
                    client.execute(root, action)
                self.assertEqual(transport.call_count, 1)
            self.assertTrue((root / "issue-a.started.json").exists())
            self.assertFalse((root / "token.secret").exists())

    def test_one_time_secret_is_private_and_not_in_completion_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture(directory)
            action = {"action_id": "issue-a", "owner": "auth", "operation": "issue", "parameters": {}, "secret_output": "token.secret"}
            token = "fixture-only-opaque-credential"
            response = {"status": 200, "body": {"state": "completed", "result": {"credential_id": "owned-id", "secret": token}}}
            reply = type("Reply", (), {"returncode": 0, "stdout": json.dumps(response)})()
            with patch("client.subprocess.run", return_value=reply) as transport:
                self.assertTrue(client.execute(root, action)["secret_saved"])
                self.assertEqual(client.execute(root, action)["state"], "completed_without_dispatch")
                self.assertEqual(transport.call_count, 1)
            self.assertEqual((root / "token.secret").stat().st_mode & 0o777, 0o600)
            self.assertNotIn(token, (root / "issue-a.completed.json").read_text())

if __name__ == "__main__":
    unittest.main()
