"""Public verifier preflight: stale sibling checkouts fail before side effects."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


VERIFY = Path(__file__).with_name("verify.py")


class VerifySourcePreflightTests(unittest.TestCase):
    def test_rejects_a_jobs_checkout_without_plugin_identity(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-source-test-") as temporary:
            root = Path(temporary)
            sources = {}
            for name, package in [
                ("auth", "lenso-auth-api-token-plugin"),
                ("jobs", "lenso-jobs-plugin"),
                ("secrets", "lenso-secrets-env-plugin"),
            ]:
                source = root / name / "crates" / package
                source.mkdir(parents=True)
                metadata = "" if name == "jobs" else (
                    f"\n[package.metadata.lenso]\nplugin-id = \"lenso.{name if name != 'auth' else 'auth.api-token'}"
                    f"{'.env' if name == 'secrets' else ''}\"\n"
                )
                (source / "Cargo.toml").write_text(
                    f"[package]\nname = \"{package}\"\nversion = \"1.0.0\"\n{metadata}",
                    encoding="utf-8",
                )
                sources[name] = source
            result = subprocess.run(
                [
                    sys.executable, str(VERIFY), "--cli", str(root / "not-a-cli"),
                    "--auth-source", str(sources["auth"]),
                    "--jobs-source", str(sources["jobs"]),
                    "--secrets-source", str(sources["secrets"]),
                ],
                env=os.environ | {"LENSO_REFERENCE_DATABASE_URL": "postgresql://invalid"},
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("lenso.jobs", result.stderr)
            self.assertIn("package.metadata.lenso", result.stderr)
            self.assertNotIn("FileNotFoundError", result.stderr)


if __name__ == "__main__":
    unittest.main()
