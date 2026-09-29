"""Credential handoff collision, permissions, and best-effort cleanup checks."""

import json
import os
import select
import signal
import stat
import subprocess
import sys
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
            with self.assertRaisesRegex(TimeoutError, "attempting to remove"):
                with browser_handoff(path, "dummy-token", "http://localhost") as handoff:
                    handoff.wait_for_removal(timeout=0, poll_interval=0)
            self.assertFalse(path.exists())

    @unittest.skipUnless(os.name == "posix", "SIGTERM handler requires POSIX")
    def test_keyboard_interrupt_cleans_up_and_restores_sigterm_handler(self):
        with tempfile.TemporaryDirectory(prefix="lenso-browser-handoff-") as temporary:
            path = Path(temporary) / "handoff.json"
            previous = signal.getsignal(signal.SIGTERM)
            def prior_handler(_signum, _frame):
                return None

            signal.signal(signal.SIGTERM, prior_handler)
            try:
                with self.assertRaises(KeyboardInterrupt):
                    with browser_handoff(path, "dummy-token", "http://localhost"):
                        self.assertIsNot(signal.getsignal(signal.SIGTERM), prior_handler)
                        raise KeyboardInterrupt
                self.assertIs(signal.getsignal(signal.SIGTERM), prior_handler)
            finally:
                signal.signal(signal.SIGTERM, previous)
            self.assertFalse(path.exists())

    @unittest.skipUnless(os.name == "posix", "SIGTERM handoff requires POSIX")
    def test_sigterm_child_exits_after_removing_handoff(self):
        with tempfile.TemporaryDirectory(prefix="lenso-browser-handoff-") as temporary:
            path = Path(temporary) / "handoff.json"
            child = (
                "import sys\n"
                "from browser_handoff import browser_handoff\n"
                "with browser_handoff(sys.argv[1], 'dummy-token', 'http://localhost') "
                "as handoff:\n"
                "    print('ready', flush=True)\n"
                "    handoff.wait_for_removal(timeout=20, poll_interval=0.01)\n"
            )
            process = subprocess.Popen(
                [sys.executable, "-c", child, str(path)],
                cwd=Path(__file__).resolve().parent,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            try:
                readable, _, _ = select.select([process.stdout], [], [], 5)
                self.assertTrue(readable, "child did not publish a handoff in time")
                self.assertEqual(process.stdout.readline().strip(), "ready")
                self.assertTrue(path.is_file())
                process.send_signal(signal.SIGTERM)
                _, errors = process.communicate(timeout=5)
                self.assertEqual(process.returncode, 128 + signal.SIGTERM, errors)
                self.assertFalse(path.exists())
            finally:
                if process.poll() is None:
                    process.kill()
                process.communicate(timeout=5)

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
