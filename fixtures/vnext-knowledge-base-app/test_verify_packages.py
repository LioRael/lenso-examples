"""Package-only verifier input and command-path checks."""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

VERIFY = Path(__file__).with_name("verify.py")


class VerifyPackagePreflightTests(unittest.TestCase):
    def run_verify(self, *arguments, env=None):
        return subprocess.run(
            [sys.executable, str(VERIFY), *map(str, arguments)],
            env=os.environ
            | {
                "LENSO_REFERENCE_DATABASE_URL": "postgresql://invalid",
                "CARGO_HOME": "/scratch/test-cargo-home",
            }
            | (env or {}),
            capture_output=True,
            text=True,
            check=False,
        )

    def package_arguments(self, root):
        snapshot = root / "snapshot.json"
        trust = root / "trust.json"
        snapshot.write_text("{}", encoding="utf-8")
        trust.write_text("{}", encoding="utf-8")
        arguments = ["--package-only", "--linked-snapshot", snapshot, "--trust", trust]
        for name in ("auth", "jobs", "secrets"):
            archive = root / f"{name}-1.2.3.crate"
            archive.write_bytes(b"not a valid crate")
            arguments.extend((f"--{name}-version", "1.2.3", f"--{name}-crate", archive))
        return arguments

    def test_source_mode_keeps_required_sources(self):
        result = self.run_verify("--cli", "/no/cli")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "source mode requires --auth-source, --jobs-source, --secrets-source",
            result.stderr,
        )

    def test_package_mode_requires_complete_input_set(self):
        result = self.run_verify("--package-only", "--cli", "/no/cli")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--linked-snapshot", result.stderr)
        self.assertIn("--secrets-crate", result.stderr)

    def test_package_mode_rejects_source_checkout(self):
        result = self.run_verify("--package-only", "--auth-source", "/source")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cannot use source checkouts: --auth-source", result.stderr)

    def test_source_mode_rejects_package_flags(self):
        result = self.run_verify("--auth-version", "1.2.3")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("package inputs require --package-only", result.stderr)

    def test_package_mode_rejects_non_exact_version(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-package-test-") as temporary:
            arguments = self.package_arguments(Path(temporary))
            position = arguments.index("--jobs-version") + 1
            arguments[position] = "latest"
            result = self.run_verify(*arguments)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "--jobs-version must be an exact Cargo version", result.stderr
            )

    def test_package_mode_rejects_detached_operator(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-package-test-") as temporary:
            arguments = self.package_arguments(Path(temporary))
            result = self.run_verify(*arguments, "--auth-operator", "/untrusted/operator")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unrecognized arguments: --auth-operator", result.stderr)

    def test_package_mode_requires_sandbox_cargo_home_before_adoption(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-package-test-") as temporary:
            arguments = self.package_arguments(Path(temporary))
            result = self.run_verify(*arguments, env={"CARGO_HOME": ""})
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires a sandbox-local CARGO_HOME before adoption", result.stderr)

    def test_complete_package_inputs_reach_exact_signed_adoption(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-package-test-") as temporary:
            root = Path(temporary)
            arguments = self.package_arguments(root)
            capture = root / "cli-arguments.txt"
            cli = root / "cli"
            cli.write_text(
                '#!/bin/sh\nprintf "%s\\n" "$@" > "$LENSO_VERIFY_CAPTURE"\nexit 29\n',
                encoding="utf-8",
            )
            cli.chmod(0o700)
            result = self.run_verify(
                "--cli", cli, *arguments, env={"LENSO_VERIFY_CAPTURE": str(capture)}
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(capture.is_file(), result.stderr)
            invoked = capture.read_text(encoding="utf-8").splitlines()
            self.assertEqual(invoked[:2], ["app", "add"])
            self.assertIn("lenso.auth.api-token@1.2.3", invoked)
            self.assertIn("--linked-snapshot", invoked)
            self.assertIn("--trust", invoked)
            self.assertIn("--crate", invoked)
            self.assertNotIn("--auth-source", invoked)

    @unittest.skipUnless(
        os.environ.get("LENSO_VERIFY_REAL_CLI"), "requires a built Lenso CLI"
    )
    def test_real_cli_rejects_malformed_snapshot_before_operator_use(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-package-test-") as temporary:
            arguments = self.package_arguments(Path(temporary))
            result = self.run_verify(
                "--cli", os.environ["LENSO_VERIFY_REAL_CLI"], *arguments
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing field `catalog_id`", result.stderr)
            self.assertNotIn("auth_operator_and_credentials", result.stderr)


if __name__ == "__main__":
    unittest.main()
