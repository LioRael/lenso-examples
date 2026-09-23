"""The source-deleted Host receives only disposable runtime paths and secrets."""

import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch

from runtime_environment import build_runtime_environment


class RuntimeEnvironmentTests(unittest.TestCase):
    def test_host_paths_are_private_and_environment_is_allowlisted(self):
        with tempfile.TemporaryDirectory(prefix="lenso-runtime-env-") as temporary:
            root = Path(temporary)
            with patch.dict(os.environ, {"PATH": "/ambient/bin", "HOST_TOKEN": "ambient"}):
                environment = build_runtime_environment(
                    root, "postgresql://dummy@127.0.0.1/test", "signing", "pepper"
                )

            self.assertEqual(
                environment,
                {
                    "PATH": str(root / "no-tools"),
                    "HOME": str(root / "runtime-home"),
                    "TMPDIR": str(root / "runtime-tmp"),
                    "LENSO_REFERENCE_DATABASE_URL": "postgresql://dummy@127.0.0.1/test",
                    "LENSO_AUTH_SIGNING_SECRET": "signing",
                    "LENSO_AUTH_TOKEN_PEPPER": "pepper",
                },
            )
            self.assertFalse((root / "no-tools").exists())
            for name in ("runtime-home", "runtime-tmp"):
                directory = root / name
                self.assertTrue(directory.is_dir())
                self.assertEqual(stat.S_IMODE(directory.stat().st_mode), 0o700)


if __name__ == "__main__":
    unittest.main()
