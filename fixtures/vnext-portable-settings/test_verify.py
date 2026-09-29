import unittest
from pathlib import Path
from unittest.mock import patch

import verify


class FrameworkRevisionTests(unittest.TestCase):
    def test_recorded_default_is_preserved(self):
        self.assertEqual(verify.DEFAULT_FRAMEWORK_SHA,
                         "6c88aeda06c5da7aed9318863fe2eba189b6c236")

    def test_explicit_exact_clean_candidate_is_accepted(self):
        revision = "3e56a3f34ef2f71f9b7a5d1e141e1d4c8d8de184"
        with patch.object(verify.subprocess, "check_output", side_effect=[revision + "\n", ""]):
            self.assertEqual(verify.verify_framework(Path("/candidate"), revision), revision)

    def test_short_or_symbolic_revision_is_rejected_before_git(self):
        for revision in ["", "main", "6c88aeda", "x" * 40]:
            with self.subTest(revision=revision):
                with patch.object(verify.subprocess, "check_output") as git:
                    with self.assertRaises(SystemExit):
                        verify.verify_framework(Path("/candidate"), revision)
                    git.assert_not_called()

    def test_wrong_head_is_rejected(self):
        with patch.object(verify.subprocess, "check_output", side_effect=["0" * 40, ""]):
            with self.assertRaises(SystemExit):
                verify.verify_framework(Path("/candidate"), verify.DEFAULT_FRAMEWORK_SHA)

    def test_tracked_changes_are_rejected(self):
        with patch.object(verify.subprocess, "check_output",
                          side_effect=[verify.DEFAULT_FRAMEWORK_SHA, " M Cargo.toml\n"]):
            with self.assertRaises(SystemExit):
                verify.verify_framework(Path("/candidate"), verify.DEFAULT_FRAMEWORK_SHA)

    def test_untracked_source_is_included_and_rejected(self):
        with patch.object(verify.subprocess, "check_output",
                          side_effect=[verify.DEFAULT_FRAMEWORK_SHA, "?? crates/lenso-kernel/build.rs\n"]) as git:
            with self.assertRaises(SystemExit):
                verify.verify_framework(Path("/candidate"), verify.DEFAULT_FRAMEWORK_SHA)
            self.assertIn("--untracked-files=all", git.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
