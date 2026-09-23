"""Credential handoff never overwrites an existing path or leaves a timed-out token."""

import json
import stat
import tempfile
import unittest
from pathlib import Path

from browser_handoff import browser_handoff


class BrowserHandoffTests(unittest.TestCase):
    def test_private_file_waits_for_manual_removal(self):
        with tempfile.TemporaryDirectory(prefix="lenso-browser-handoff-") as temporary:
            path = Path(temporary) / "handoff.json"
            with browser_handoff(path, "dummy-token", "http://127.0.0.1:1234") as handoff:
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
                self.assertEqual(
                    json.loads(path.read_text(encoding="utf-8")),
                    {"token": "dummy-token", "url": "http://127.0.0.1:1234"},
                )
                path.unlink()
                handoff.wait_for_removal(timeout=0.01, poll_interval=0.001)
            self.assertFalse(path.exists())

    def test_existing_file_and_symlink_are_not_overwritten(self):
        with tempfile.TemporaryDirectory(prefix="lenso-browser-handoff-") as temporary:
            root = Path(temporary)
            existing = root / "existing.json"
            existing.write_text("keep", encoding="utf-8")
            with self.assertRaises(FileExistsError):
                with browser_handoff(existing, "dummy-token", "http://localhost"):
                    pass
            self.assertEqual(existing.read_text(encoding="utf-8"), "keep")

            link = root / "link.json"
            link.symlink_to(existing)
            with self.assertRaises(FileExistsError):
                with browser_handoff(link, "dummy-token", "http://localhost"):
                    pass
            self.assertTrue(link.is_symlink())
            self.assertEqual(existing.read_text(encoding="utf-8"), "keep")

    def test_timeout_removes_only_its_own_file(self):
        with tempfile.TemporaryDirectory(prefix="lenso-browser-handoff-") as temporary:
            path = Path(temporary) / "handoff.json"
            with self.assertRaisesRegex(TimeoutError, "credential file will be removed"):
                with browser_handoff(path, "dummy-token", "http://localhost") as handoff:
                    handoff.wait_for_removal(timeout=0, poll_interval=0)
            self.assertFalse(path.exists())

    def test_replaced_path_is_not_removed(self):
        with tempfile.TemporaryDirectory(prefix="lenso-browser-handoff-") as temporary:
            root = Path(temporary)
            path = root / "handoff.json"
            replacement = root / "replacement.json"
            replacement.write_text("keep", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "path was replaced"):
                with browser_handoff(path, "dummy-token", "http://localhost") as handoff:
                    path.unlink()
                    path.symlink_to(replacement)
                    handoff.wait_for_removal(timeout=0, poll_interval=0)
            self.assertTrue(path.is_symlink())
            self.assertEqual(replacement.read_text(encoding="utf-8"), "keep")

    def test_non_sticky_shared_parent_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="lenso-browser-handoff-") as temporary:
            root = Path(temporary)
            root.chmod(0o777)
            try:
                with self.assertRaisesRegex(ValueError, "writable without a sticky bit"):
                    with browser_handoff(root / "handoff.json", "dummy-token", "http://localhost"):
                        pass
                self.assertFalse((root / "handoff.json").exists())
            finally:
                root.chmod(0o700)


if __name__ == "__main__":
    unittest.main()
