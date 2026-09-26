"""Public verifier preflight: stale sibling checkouts fail before side effects."""

import ast
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch


VERIFY = Path(__file__).with_name("verify.py")


class VerifySourcePreflightTests(unittest.TestCase):
    def test_candidate_cargo_context_stays_out_of_provider_operators(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-source-test-") as temporary:
            root = Path(temporary)
            manifest = VERIFY.parent / "project" / "app" / "notes-web" / "Cargo.toml"
            with manifest.open("rb") as stream:
                direct_dependencies = tomllib.load(stream)["dependencies"]
            module = ast.parse(VERIFY.read_text(encoding="utf-8"))
            patch_packages = next(
                ast.literal_eval(statement.value)
                for statement in module.body
                if isinstance(statement, ast.Assign)
                and any(
                    isinstance(target, ast.Name) and target.id == "FRAMEWORK_PATCH_PACKAGES"
                    for target in statement.targets
                )
            )
            framework = root / "framework"
            for name in patch_packages:
                crate = framework / "crates" / name
                crate.mkdir(parents=True)
                version = direct_dependencies.get(name, "=0.1.0").removeprefix("=")
                (crate / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion = "{version}"\n',
                    encoding="utf-8",
                )
            provider = root / "provider" / "crates" / "lenso-capability-agent-tool-provider"
            provider.mkdir(parents=True)
            (provider / "Cargo.toml").write_text(
                '[package]\nname = "lenso-capability-agent-tool-provider"\nversion = "0.3.0"\n'
                '[package.metadata.lenso.contract]\nprojection = "rust-runtime"\n',
                encoding="utf-8",
            )
            sources = {}
            for name, package, plugin_id in [
                ("auth", "lenso-auth-api-token-plugin", "lenso.auth.api-token"),
                ("jobs", "lenso-jobs-plugin", "lenso.jobs"),
                ("secrets", "lenso-secrets-env-plugin", "lenso.secrets.env"),
            ]:
                source = root / name / "crates" / package
                source.mkdir(parents=True)
                (source / "Cargo.toml").write_text(
                    f'[package]\nname = "{package}"\nversion = "0.1.0"\n'
                    f'[package.metadata.lenso]\nplugin-id = "{plugin_id}"\n',
                    encoding="utf-8",
                )
                sources[name] = source

            bin_directory = root / "bin"
            bin_directory.mkdir()
            cli = bin_directory / "lenso-test-cli"
            cli.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            cli.chmod(0o700)
            auth_operator = bin_directory / "api-token-operator"
            auth_operator.write_text(
                '#!/bin/sh\n'
                'printf "%s|%s\\n" "$CARGO_HOME" "$*" >> "$LENSO_OPERATOR_CAPTURE"\n'
                'if [ "$1" = public-key ]; then printf "test-public-key\\n"; fi\n',
                encoding="utf-8",
            )
            auth_operator.chmod(0o700)
            cargo = bin_directory / "cargo"
            cargo.write_text(
                '#!/bin/sh\n'
                'printf "%s|%s\\n" "$CARGO_HOME" "$*" >> "$LENSO_CARGO_CAPTURE"\n'
                'if [ "$1" = build ]; then\n'
                '  while [ "$#" -gt 0 ]; do\n'
                '    if [ "$1" = --target-dir ]; then\n'
                '      mkdir -p "$2/debug/examples"\n'
                '      cp "$LENSO_FAKE_AUTH_OPERATOR" "$2/debug/examples/api-token-operator"\n'
                '      exit 0\n'
                '    fi\n'
                '    shift\n'
                '  done\n'
                '  exit 31\n'
                'fi\n'
                'case " $* " in\n'
                '  *" public-key "*) printf "test-public-key\\n" ;;\n'
                '  *" knowledge-operator "*) exit 29 ;;\n'
                'esac\n',
                encoding="utf-8",
            )
            cargo.chmod(0o700)
            original_cargo_home = root / "original-cargo-home"
            original_cargo_home.mkdir()
            capture = root / "cargo-calls.txt"
            operator_capture = root / "operator-calls.txt"
            result = subprocess.run(
                [
                    sys.executable, str(VERIFY), "--cli", str(cli),
                    "--framework-source", str(framework),
                    "--tool-provider-source", str(provider),
                    "--auth-source", str(sources["auth"]),
                    "--jobs-source", str(sources["jobs"]),
                    "--secrets-source", str(sources["secrets"]),
                ],
                env=os.environ | {
                    "PATH": f"{bin_directory}{os.pathsep}{os.environ['PATH']}",
                    "CARGO_HOME": str(original_cargo_home),
                    "LENSO_CARGO_CAPTURE": str(capture),
                    "LENSO_OPERATOR_CAPTURE": str(operator_capture),
                    "LENSO_FAKE_AUTH_OPERATOR": str(auth_operator),
                    "LENSO_REFERENCE_DATABASE_URL": "postgresql://invalid",
                },
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(capture.is_file(), result.stderr)
            self.assertTrue(operator_capture.is_file(), result.stderr)
            calls = [line.split("|", 1) for line in capture.read_text().splitlines()]
            auth_builds = [command for home, command in calls if command.startswith("build ")
                           and "--example api-token-operator" in command]
            self.assertEqual(len(auth_builds), 1, calls)
            self.assertFalse(any(command.startswith("run ") and "api-token-operator" in command
                                 for _, command in calls), calls)
            jobs_homes = [home for home, command in calls if "jobs-operator" in command]
            self.assertTrue(jobs_homes, calls)
            self.assertEqual(jobs_homes, [str(original_cargo_home.resolve())] * len(jobs_homes))
            operator_calls = [line.split("|", 1) for line in operator_capture.read_text().splitlines()]
            self.assertEqual([command.split(" ", 1)[0] for _, command in operator_calls],
                             ["setup", "public-key"])
            self.assertEqual([home for home, _ in operator_calls],
                             [str(original_cargo_home.resolve())] * len(operator_calls))
            update = [
                (index, home) for index, (home, command) in enumerate(calls)
                if command.startswith("update --offline --manifest-path ")
            ]
            knowledge = [
                (index, home) for index, (home, command) in enumerate(calls)
                if "knowledge-operator" in command
            ]
            self.assertEqual(len(update), 1, calls)
            self.assertEqual(len(knowledge), 1, calls)
            self.assertLess(update[0][0], knowledge[0][0])
            self.assertIn("candidate-cargo-home", update[0][1])
            self.assertEqual(update[0][1], knowledge[0][1])

    def test_rejects_framework_source_that_does_not_match_exact_pin(self):
        with tempfile.TemporaryDirectory(prefix="lenso-kb-source-test-") as temporary:
            root = Path(temporary)
            lenso = root / "framework" / "crates" / "lenso"
            lenso.mkdir(parents=True)
            (lenso / "Cargo.toml").write_text(
                '[package]\nname = "lenso"\nversion = "0.5.25"\n', encoding="utf-8"
            )
            result = subprocess.run(
                [
                    sys.executable, str(VERIFY), "--framework-source", str(root / "framework"),
                    "--tool-provider-source", str(root / "provider"),
                ],
                env=os.environ | {"LENSO_REFERENCE_DATABASE_URL": "postgresql://invalid"},
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("candidate lenso@0.5.25 does not match", result.stderr)
            self.assertNotIn("FileNotFoundError", result.stderr)

    def test_rejects_tool_provider_without_runtime_projection(self):
        function = next(
            node for node in ast.parse(VERIFY.read_text()).body
            if isinstance(node, ast.FunctionDef) and node.name == "candidate_framework_inputs"
        )
        with tempfile.TemporaryDirectory(prefix="lenso-kb-source-test-") as temporary:
            root = Path(temporary)
            provider = root / "provider"
            provider.mkdir()
            (provider / "Cargo.toml").write_text(
                '[package]\nname = "lenso-capability-agent-tool-provider"\nversion = "0.3.0"\n'
            )
            parser = argparse.ArgumentParser()
            namespace = {
                "Path": Path, "tomllib": tomllib, "parser": parser,
                "fixture": VERIFY.parent, "FRAMEWORK_PATCH_PACKAGES": (),
                "args": argparse.Namespace(
                    framework_source=str(root / "framework"),
                    tool_provider_source=str(provider),
                ),
            }
            exec(compile(ast.Module(body=[function], type_ignores=[]), str(VERIFY), "exec"), namespace)
            with patch.object(parser, "error", side_effect=ValueError) as error:
                with self.assertRaises(ValueError):
                    namespace["candidate_framework_inputs"]()
                error.assert_called_once_with(
                    "candidate Agent Tool Provider must provide a rust-runtime contract"
                )

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
