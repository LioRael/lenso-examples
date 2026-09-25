"""Package-only verifier input and command-path checks."""

import ast
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from package_cargo_environment import package_build_environment

VERIFY = Path(__file__).with_name("verify.py")


class VerifyPackagePreflightTests(unittest.TestCase):
    def test_opt_in_task_cargo_target_is_offline_and_shared_across_builds(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "task-target"
            with patch.dict(os.environ, {
                "PATH": "/usr/bin",
                "CARGO_HOME": str(root / "cargo-home"),
                "LENSO_REFERENCE_CARGO_TARGET_DIR": str(target),
            }, clear=True):
                first = package_build_environment(root / "first-home")
                second = package_build_environment(root / "second-home")
            self.assertEqual(first["CARGO_TARGET_DIR"], str(target))
            self.assertEqual(second["CARGO_TARGET_DIR"], str(target))
            self.assertEqual(first["CARGO_NET_OFFLINE"], "true")
            self.assertEqual(first["CARGO_HOME"], str(root / "cargo-home"))
            self.assertNotEqual(first["HOME"], second["HOME"])
            self.assertTrue(target.is_dir())

    def test_task_cargo_target_rejects_relative_or_symlink_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real = root / "real"
            real.mkdir()
            link = root / "link"
            link.symlink_to(real, target_is_directory=True)
            for target in ("relative-target", "/", str(link)):
                with self.subTest(target=target), patch.dict(os.environ, {
                    "CARGO_HOME": str(root / "cargo-home"),
                    "LENSO_REFERENCE_CARGO_TARGET_DIR": target,
                }, clear=True):
                    with self.assertRaisesRegex(RuntimeError, "absolute non-symlink task target"):
                        package_build_environment(root / "home")

    def test_adopted_operators_use_shared_target_only_when_opted_in(self):
        module = ast.parse(VERIFY.read_text(encoding="utf-8"))
        function = next(
            node for node in module.body
            if isinstance(node, ast.FunctionDef) and node.name == "build_adopted_operator"
        )
        compiled = compile(ast.Module(body=[function], type_ignores=[]), str(VERIFY), "exec")

        for shared in (False, True):
            with self.subTest(shared=shared), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                project = root / "project"
                project.mkdir()
                build_root = root / "build"
                source = root / "adopted-source"
                source.mkdir()
                (source / "Cargo.toml").write_text('[package]\nname = "operator"\n', encoding="utf-8")
                (source / "Cargo.lock").write_text("locked\n", encoding="utf-8")
                lock_bytes = json.dumps({
                    "crate_digest": "sha256:crate",
                    "source_digest": "sha256:source",
                }).encode()
                builds = []

                def run(command, *, cwd, env):
                    self.assertEqual(cwd, project)
                    builds.append((command, env))
                    target = Path(command[command.index("--target-dir") + 1])
                    example = command[command.index("--example") + 1]
                    binary = target / "debug" / "examples" / example
                    binary.parent.mkdir(parents=True, exist_ok=True)
                    binary.write_bytes(example.encode())

                def sha256_file(path):
                    return hashlib.sha256(path.read_bytes()).hexdigest()

                namespace = {
                    "Path": Path,
                    "OPERATOR_EXAMPLES": {
                        "auth": "api-token-operator",
                        "jobs": "jobs-operator",
                    },
                    "PLUGIN_IDS": {
                        "auth": "lenso.auth.api-token",
                        "jobs": "lenso.jobs",
                    },
                    "adopted_operator_source": lambda *_: (source, lock_bytes),
                    "package_build_environment": package_build_environment,
                    "run": run,
                    "stat": stat,
                    "os": os,
                    "shutil": shutil,
                    "sha256_file": sha256_file,
                    "json": json,
                }
                exec(compiled, namespace)
                environment = {
                    "PATH": "/usr/bin",
                    "CARGO_HOME": str(root / "cargo-home"),
                }
                if shared:
                    environment["LENSO_REFERENCE_CARGO_TARGET_DIR"] = str(root / "shared")
                with patch.dict(os.environ, environment, clear=True):
                    for name in ("auth", "jobs"):
                        output, receipt = namespace["build_adopted_operator"](
                            project, build_root, name, "1.2.3", root / f"{name}.crate"
                        )
                        self.assertEqual(output.read_bytes(), namespace["OPERATOR_EXAMPLES"][name].encode())
                        self.assertEqual(receipt["operator_sha256"], sha256_file(output))

                self.assertEqual(len(builds), 2)
                for name, (command, cargo_environment) in zip(("auth", "jobs"), builds):
                    expected = root / "shared" if shared else build_root / "operator-targets" / name
                    self.assertEqual(Path(command[command.index("--target-dir") + 1]), expected)
                    self.assertEqual(cargo_environment.get("CARGO_TARGET_DIR"), str(expected) if shared else None)
                    self.assertIn("--locked", command)
                    self.assertIn("--offline", command)
                    self.assertEqual(cargo_environment["CARGO_NET_OFFLINE"], "true")
                    self.assertEqual(cargo_environment["CARGO_HOME"], environment["CARGO_HOME"])
                    self.assertEqual(cargo_environment["HOME"], str(build_root / "operator-home" / name))

    def test_built_app_check_uses_distribution_root_not_intent(self):
        source = VERIFY.read_text()
        roots = []
        for node in ast.walk(ast.parse(source)):
            if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name) or node.func.id != "run":
                continue
            if not node.args or not isinstance(node.args[0], ast.List):
                continue
            command = node.args[0].elts
            if len(command) < 5 or not all(
                isinstance(command[index], ast.Constant) and command[index].value == value
                for index, value in ((1, "app"), (2, "check"), (3, "--root"))
            ):
                continue
            root = command[4]
            self.assertIsInstance(root, ast.Call)
            self.assertIsInstance(root.func, ast.Name)
            self.assertEqual(root.func.id, "str")
            self.assertIsInstance(root.args[0], ast.Name)
            roots.append(root.args[0].id)
        self.assertCountEqual(
            roots,
            ("distribution", "removed_distribution", "upgraded_distribution", "lifecycle_root"),
        )
        self.assertIn(
            '[cli, "app", "show", "--root", str(intent), "--json"]',
            source,
        )

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

    def test_package_mode_rejects_framework_candidate_source(self):
        result = self.run_verify("--package-only", "--framework-source", "/source")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cannot use source checkouts: --framework-source", result.stderr)

    def test_source_mode_requires_both_candidate_inputs(self):
        result = self.run_verify("--framework-source", "/source")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "requires both --framework-source and --tool-provider-source",
            result.stderr,
        )

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

    def test_upgrade_requires_both_exact_distinct_inputs(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-package-test-") as temporary:
            root = Path(temporary)
            arguments = self.package_arguments(root)
            upgrade = root / "secrets-1.2.4.crate"
            upgrade.write_bytes(b"second release probe")
            cases = (
                (["--secrets-upgrade-version", "1.2.4"], "must be supplied together"),
                (["--secrets-upgrade-version", "latest", "--secrets-upgrade-crate", upgrade], "must be an exact Cargo version"),
                (["--secrets-upgrade-version", "1.2.3", "--secrets-upgrade-crate", upgrade], "must differ"),
            )
            for flags, expected in cases:
                with self.subTest(flags=flags):
                    result = self.run_verify(*arguments, *flags)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(expected, result.stderr)

    def test_source_mode_rejects_upgrade_inputs(self):
        result = self.run_verify("--secrets-upgrade-version", "1.2.4")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("package inputs require --package-only", result.stderr)

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
