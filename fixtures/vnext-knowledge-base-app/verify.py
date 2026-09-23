#!/usr/bin/env python3
"""Build and verify the source-deleted knowledge base reference application."""
import argparse
import base64
import hashlib
import json
import os
import platform
import queue
import re
import secrets
import shutil
import signal
import stat
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from contextlib import contextmanager
from pathlib import Path

import tomllib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--cli", default="lenso")
parser.add_argument("--auth-source", help="local API Token Auth source crate (default mode)")
parser.add_argument("--jobs-source", help="local Jobs source crate (default mode)")
parser.add_argument("--secrets-source", help="local environment Secrets source crate (default mode)")
parser.add_argument(
    "--package-only", action="store_true",
    help="adopt Auth, Jobs, and Secrets only from exact catalog-bound .crate inputs",
)
parser.add_argument("--linked-snapshot", help="signed linked Cargo snapshot for package-only mode")
parser.add_argument("--trust", help="public trust configuration for package-only mode")
for provider in ("auth", "jobs", "secrets"):
    parser.add_argument(f"--{provider}-version", help=f"exact {provider} Plugin package version")
    parser.add_argument(f"--{provider}-crate", help=f"signed {provider} .crate archive")
parser.add_argument(
    "--browser-handoff",
    type=Path,
    help="optional temporary JSON handoff; delete it after browser acceptance to continue",
)
parser.add_argument(
    "--web-client-package",
    help="optional @lenso/web-client .tgz used to rebuild and typecheck the React UI",
)
args = parser.parse_args()
cli = str(Path(shutil.which(args.cli) or args.cli).absolute())
fixture = Path(__file__).resolve().parent
database_url = os.environ.get("LENSO_REFERENCE_DATABASE_URL")
if not database_url:
    parser.error("LENSO_REFERENCE_DATABASE_URL must name a disposable PostgreSQL database")


def repository_for(crate, expected_plugin_id):
    crate = Path(crate).resolve()
    if not (crate / "Cargo.toml").is_file() or crate.parent.name != "crates":
        parser.error(f"source crate must be a crates/<package> directory: {crate}")
    with (crate / "Cargo.toml").open("rb") as manifest:
        package = tomllib.load(manifest).get("package", {})
    actual_plugin_id = package.get("metadata", {}).get("lenso", {}).get("plugin-id")
    if actual_plugin_id != expected_plugin_id:
        parser.error(
            f"{crate} must declare package.metadata.lenso.plugin-id = "
            f"{expected_plugin_id!r}; found {actual_plugin_id!r}. "
            "Select a source revision that supports linked App adoption."
        )
    return crate.parents[1]


PLUGIN_IDS = {
    "auth": "lenso.auth.api-token",
    "jobs": "lenso.jobs",
    "secrets": "lenso.secrets.env",
}
OPERATOR_EXAMPLES = {
    "auth": "api-token-operator",
    "jobs": "jobs-operator",
}
MAX_LINKED_SOURCE_FILES = 4096
MAX_LINKED_SOURCE_BYTES = 128 * 1024 * 1024


def regular_file(value, label):
    path = Path(value).resolve()
    if not path.is_file():
        parser.error(f"{label} must be an existing regular file: {path}")
    return path


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def linked_source_digest(root):
    """Match the linked Cargo source lock's digest over regular source files."""
    def fail_walk(error):
        raise error

    files = []
    total = 0
    for current, directories, names in os.walk(root, followlinks=False, onerror=fail_walk):
        directory = Path(current)
        for name in directories:
            child = directory / name
            if not stat.S_ISDIR(child.lstat().st_mode):
                raise RuntimeError(f"linked source contains a non-directory: {child}")
        if directory == root:
            directories[:] = [name for name in directories if name != "target"]
        for name in names:
            path = directory / name
            metadata = path.lstat()
            if not stat.S_ISREG(metadata.st_mode):
                raise RuntimeError(f"linked source contains a non-file: {path}")
            if path in (root / ".lenso-linked-source.json", root / "Cargo.lock"):
                continue
            total += metadata.st_size
            if total > MAX_LINKED_SOURCE_BYTES:
                raise RuntimeError("linked source exceeds size limit")
            files.append((path, metadata.st_size))
            if len(files) > MAX_LINKED_SOURCE_FILES:
                raise RuntimeError("linked source has too many files")
    digest = hashlib.sha256()
    for path, size in sorted(files):
        relative = path.relative_to(root).as_posix().encode("utf-8")
        contents = path.read_bytes()
        if len(contents) != size:
            raise RuntimeError(f"linked source changed while reading: {path}")
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(contents).to_bytes(8, "big"))
        digest.update(contents)
    return "sha256:" + digest.hexdigest()


def adopted_operator_source(project, name, version, archive):
    source = project / "vendor" / "lenso" / PLUGIN_IDS[name] / version
    lock_path = source / ".lenso-linked-source.json"
    lock_bytes = lock_path.read_bytes()
    if len(lock_bytes) > 4096:
        raise RuntimeError(f"{name} linked source lock exceeds size limit")
    lock = json.loads(lock_bytes)
    expected = {
        "schema_version": 1,
        "plugin_id": PLUGIN_IDS[name],
        "version": version,
        "crate_digest": "sha256:" + sha256_file(archive),
    }
    for key, value in expected.items():
        if lock.get(key) != value:
            raise RuntimeError(f"{name} adopted source {key} differs from exact .crate")
    example = source / "examples" / f"{OPERATOR_EXAMPLES[name]}.rs"
    if not stat.S_ISREG(example.lstat().st_mode):
        raise RuntimeError(f"{name} operator example is not a regular packaged source file")
    if linked_source_digest(source) != lock.get("source_digest"):
        raise RuntimeError(f"{name} adopted source differs from its verified source lock")
    archive_lock_digest = lock.get("archive_cargo_lock_digest")
    if archive_lock_digest is not None:
        cargo_lock = source / "Cargo.lock"
        if not cargo_lock.is_file() or "sha256:" + sha256_file(cargo_lock) != archive_lock_digest:
            raise RuntimeError(f"{name} signed archive Cargo.lock changed")
    return source, lock_bytes


def package_build_environment(home):
    if not os.environ.get("CARGO_HOME"):
        raise RuntimeError("--package-only requires an explicit sandbox-local CARGO_HOME")
    home.mkdir(parents=True, exist_ok=True)
    environment = {
        key: os.environ[key]
        for key in ("PATH", "CARGO_HOME", "RUSTUP_HOME")
        if key in os.environ
    }
    environment["HOME"] = str(home)
    environment["CARGO_NET_OFFLINE"] = "true"
    return environment


def build_adopted_operator(project, root, name, version, archive):
    source, lock_bytes = adopted_operator_source(project, name, version, archive)
    manifest = source / "Cargo.toml"
    example = OPERATOR_EXAMPLES[name]
    target = root / "operator-targets" / name
    cargo_environment = package_build_environment(root / "operator-home" / name)
    if not (source / "Cargo.lock").is_file():
        run(
            ["cargo", "generate-lockfile", "--offline", "--manifest-path", str(manifest)],
            cwd=project, env=cargo_environment,
        )
    run(
        [
            "cargo", "build", "--locked", "--offline", "--manifest-path", str(manifest),
            "--example", example, "--target-dir", str(target),
        ],
        cwd=project, env=cargo_environment,
    )
    _, lock_after = adopted_operator_source(project, name, version, archive)
    if lock_after != lock_bytes:
        raise RuntimeError(f"{name} adopted source lock changed during operator build")
    built = target / "debug" / "examples" / (example + (".exe" if os.name == "nt" else ""))
    if not stat.S_ISREG(built.lstat().st_mode):
        raise RuntimeError(f"{name} operator was not built as a regular file")
    output = root / "operators" / name
    output.parent.mkdir(exist_ok=True)
    shutil.copyfile(built, output)
    output.chmod(0o700)
    if sha256_file(output) != sha256_file(built):
        raise RuntimeError(f"{name} operator changed while copying")
    return output, {
        "plugin_id": PLUGIN_IDS[name],
        "version": version,
        "crate_digest": json.loads(lock_bytes)["crate_digest"],
        "source_digest": json.loads(lock_bytes)["source_digest"],
        "cargo_lock_sha256": sha256_file(source / "Cargo.lock"),
        "operator_sha256": sha256_file(output),
    }


def package_inputs():
    source_flags = [f"--{name}-source" for name in PLUGIN_IDS if getattr(args, f"{name}_source")]
    if source_flags:
        parser.error(f"--package-only cannot use source checkouts: {', '.join(source_flags)}")
    required = ["linked_snapshot", "trust"]
    for name in PLUGIN_IDS:
        required.extend((f"{name}_version", f"{name}_crate"))
    missing = [f"--{name.replace('_', '-')}" for name in required if not getattr(args, name)]
    if missing:
        parser.error(f"--package-only requires {', '.join(missing)}")
    if not os.environ.get("CARGO_HOME"):
        parser.error("--package-only requires a sandbox-local CARGO_HOME before adoption")

    snapshot = regular_file(args.linked_snapshot, "--linked-snapshot")
    trust = regular_file(args.trust, "--trust")
    releases = {}
    for name, plugin_id in PLUGIN_IDS.items():
        version = getattr(args, f"{name}_version")
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
            parser.error(f"--{name}-version must be an exact Cargo version: {version!r}")
        archive = regular_file(getattr(args, f"{name}_crate"), f"--{name}-crate")
        if archive.suffix != ".crate":
            parser.error(f"--{name}-crate must name a .crate archive: {archive}")
        releases[name] = (f"{plugin_id}@{version}", archive)

    return snapshot, trust, releases


if args.package_only:
    repositories = None
    linked_snapshot, trust, releases = package_inputs()
else:
    package_flags = [
        "linked_snapshot", "trust",
        *(f"{name}_{field}" for name in PLUGIN_IDS for field in ("version", "crate")),
    ]
    supplied = [f"--{name.replace('_', '-')}" for name in package_flags if getattr(args, name)]
    if supplied:
        parser.error(f"package inputs require --package-only: {', '.join(supplied)}")
    missing = [f"--{name}-source" for name in PLUGIN_IDS if not getattr(args, f"{name}_source")]
    if missing:
        parser.error(f"source mode requires {', '.join(missing)}")
    repositories = {
        name: repository_for(getattr(args, f"{name}_source"), plugin_id)
        for name, plugin_id in PLUGIN_IDS.items()
    }


def run(command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def operator_environment(**values):
    return os.environ | values


def launch(cli, distribution, root, environment, app_root=None):
    command = [cli, "app", "start", "--from", str(distribution)]
    if app_root is not None:
        command.extend(["--root", str(app_root)])
    process = subprocess.Popen(
        command,
        env=environment,
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    events = queue.Queue()
    transcript = []

    def read_output():
        for line in process.stdout:
            transcript.append(line)
            events.put(line)
        events.put(None)

    reader = threading.Thread(target=read_output, daemon=True)
    reader.start()
    try:
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            line = events.get(timeout=max(0.1, deadline - time.monotonic()))
            if line is None:
                raise RuntimeError("Host exited before readiness")
            match = re.search(r"Listening on (http://\S+)", line)
            if match:
                return process, reader, transcript, match[1]
        raise RuntimeError("Host did not report Web readiness")
    except BaseException:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        reader.join(timeout=2)
        print("".join(transcript))
        raise


def stop(process, reader, transcript):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        raise
    reader.join(timeout=2)
    print("".join(transcript))
    assert process.returncode == 0


def http_json(url, method="GET", body=None, token=None, expected=200):
    headers = {}
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status == expected
        return json.load(response)


def expect_http_error(url, code, method="GET", body=None, token=None):
    try:
        http_json(url, method=method, body=body, token=token)
    except urllib.error.HTTPError as error:
        assert error.code == code, error.read().decode()
    else:
        raise AssertionError(f"request must return HTTP {code}: {method} {url}")


def tree_logical_bytes(directory):
    """Count regular-file bytes without following links outside the consumer."""
    if not directory.exists():
        return 0
    total = 0
    for current, directories, files in os.walk(directory, followlinks=False):
        directories[:] = [
            name for name in directories if not (Path(current) / name).is_symlink()
        ]
        for name in files:
            path = Path(current) / name
            if not path.is_symlink() and path.is_file():
                total += path.stat().st_size
    return total


def command_version(command):
    return subprocess.run(
        command, check=True, capture_output=True, text=True
    ).stdout.strip()


with tempfile.TemporaryDirectory(prefix="lenso-knowledge-base-") as temporary:
    root = Path(temporary)
    source = root / "source"
    phases = []

    @contextmanager
    def measured(stage, name):
        before = tree_logical_bytes(root)
        started = time.monotonic()
        yield
        elapsed = time.monotonic() - started
        phases.append({
            "stage": stage,
            "name": name,
            "seconds": round(elapsed, 3),
            "consumer_disk_delta_bytes": tree_logical_bytes(root) - before,
        })

    with measured("consumer_preparation", "fixture_copy"):
        shutil.copytree(
            fixture,
            source,
            ignore=shutil.ignore_patterns(
                "target", ".lenso", "dist", "node_modules", "vendor", "generated", "__pycache__"
            ),
        )
    if args.web_client_package:
        with measured("frontend_authoring", "packed_web_client_and_react_build"):
            frontend = source / "project" / "frontend"
            vendor = frontend / "vendor"
            vendor.mkdir()
            shutil.copyfile(Path(args.web_client_package).resolve(), vendor / "lenso-web-client.tgz")
            run(["bun", "install", "--frozen-lockfile"], cwd=frontend)
            run(["bun", "run", "generate"], cwd=frontend)
            run(["bun", "run", "typecheck"], cwd=frontend)
            run(["bun", "run", "build"], cwd=frontend)

    project = source / "project"
    operator_receipts = None
    if args.package_only:
        with measured("consumer_preparation", "catalog_bound_crate_adoption"):
            for coordinate, archive in releases.values():
                run([
                    cli, "app", "add", "--root", str(project), "--no-install",
                    "--linked-snapshot", str(linked_snapshot), "--trust", str(trust),
                    "--crate", str(archive), coordinate,
                ])
        with measured("consumer_preparation", "adopted_source_preflight"):
            # Auth's public key is produced by its operator. Full App resolution
            # must wait for that configuration; app add has already verified
            # the signed archive, and these checks bind the adopted source to it.
            for name in OPERATOR_EXAMPLES:
                coordinate, archive = releases[name]
                adopted_operator_source(project, name, coordinate.rsplit("@", 1)[1], archive)
        with measured("plugin_candidate_setup", "crate_derived_operator_build"):
            built_operators = {}
            operator_receipts = {}
            for name in OPERATOR_EXAMPLES:
                coordinate, archive = releases[name]
                built_operators[name], operator_receipts[name] = build_adopted_operator(
                    project, root, name, coordinate.rsplit("@", 1)[1], archive
                )
        auth_operator = [str(built_operators["auth"])]
        jobs_operator = [str(built_operators["jobs"])]
        auth_operator_cwd = root
        jobs_operator_cwd = root
    else:
        candidates = source / "candidates"
        with measured("consumer_preparation", "candidate_source_copy"):
            for name, repository in repositories.items():
                shutil.copytree(
                    repository,
                    candidates / name,
                    ignore=shutil.ignore_patterns(".git", ".worktrees", "target"),
                )
        auth_source = candidates / "auth" / "crates" / "lenso-auth-api-token-plugin"
        jobs_source = candidates / "jobs" / "crates" / "lenso-jobs-plugin"
        secrets_source = candidates / "secrets" / "crates" / "lenso-secrets-env-plugin"
        with measured("consumer_preparation", "source_adoption"):
            for plugin_source in (auth_source, jobs_source, secrets_source):
                run([cli, "app", "add", "--root", str(project), "--no-install", str(plugin_source)])
        auth_operator = [
            "cargo", "run", "--locked", "-p", "lenso-auth-api-token-plugin",
            "--example", "api-token-operator", "--",
        ]
        jobs_operator = [
            "cargo", "run", "--locked", "-p", "lenso-jobs-plugin",
            "--example", "jobs-operator", "--",
        ]
        auth_operator_cwd = candidates / "auth"
        jobs_operator_cwd = candidates / "jobs"

    suffix = f"{os.getpid()}_{secrets.randbelow(1_000_000)}"
    auth_schema = f"auth_reference_{suffix}"
    jobs_schema = f"jobs_reference_{suffix}"
    signing_secret = secrets.token_urlsafe(48)
    token_pepper = secrets.token_urlsafe(48)
    auth_environment = operator_environment(
        LENSO_AUTH_DATABASE_URL=database_url,
        LENSO_AUTH_SIGNING_SECRET=signing_secret,
        LENSO_AUTH_TOKEN_PEPPER=token_pepper,
    )
    with measured("plugin_candidate_setup", "auth_operator_setup"):
        run(auth_operator + ["setup", auth_schema], cwd=auth_operator_cwd, env=auth_environment)
        public_key = subprocess.run(
            auth_operator + ["public-key"],
            cwd=auth_operator_cwd,
            env=auth_environment,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

    def issue_tokens():
        tokens = {}
        for subject in ["user-a", "user-b"]:
            output = subprocess.run(
                auth_operator
                + ["issue", auth_schema, subject, "lenso.reference.knowledge-base@1:access"],
                cwd=auth_operator_cwd,
                env=auth_environment,
                check=True,
                capture_output=True,
                text=True,
            ).stdout
            tokens[subject] = json.loads(output)["token"]
        return tokens

    with measured("plugin_candidate_setup", "jobs_operator_setup"):
        run(
            jobs_operator + ["setup", jobs_schema],
            cwd=jobs_operator_cwd,
            env=operator_environment(LENSO_JOBS_DATABASE_URL=database_url),
        )
    notes_web = project / "app" / "notes-web"
    with measured("app_authoring", "knowledge_operator_setup"):
        run(
            ["cargo", "run", "--locked", "--manifest-path", str(notes_web / "Cargo.toml"),
             "--example", "knowledge-operator", "--", "setup"],
            env=operator_environment(LENSO_KNOWLEDGE_DATABASE_URL=database_url),
        )

    (project / "plugins" / "lenso.auth.api-token" / "default.toml").write_text(
        f'''schema = "{auth_schema}"
issuer = "knowledge-reference"
assertion_public_key = "{public_key}"
database_url_secret = "auth/database-url"
assertion_signing_key_secret = "auth/signing-secret"
token_pepper_secret = "auth/token-pepper"
assertion_ttl_seconds = 300
'''
    )
    (project / "plugins" / "lenso.jobs" / "default.toml").write_text(
        f'''schema = "{jobs_schema}"
database_url_secret = "jobs/database-url"
lease_seconds = 30
retry_base_seconds = 5
retry_max_seconds = 300
queues = ["knowledge"]
producer_instances = ["lenso.reference.knowledge-excerpt/default"]
worker_instances = ["lenso.reference.knowledge-excerpt/default"]
observer_instances = ["lenso.reference.knowledge-excerpt/default"]
'''
    )
    (project / "plugins" / "lenso.secrets.env" / "default.toml").write_text(
        '''[references]
"auth/database-url" = "LENSO_REFERENCE_DATABASE_URL"
"auth/signing-secret" = "LENSO_AUTH_SIGNING_SECRET"
"auth/token-pepper" = "LENSO_AUTH_TOKEN_PEPPER"
"jobs/database-url" = "LENSO_REFERENCE_DATABASE_URL"
"knowledge/database-url" = "LENSO_REFERENCE_DATABASE_URL"
'''
    )

    excerpt = project / "app" / "excerpt"
    with measured("app_authoring", "typescript_install_and_check"):
        run(["bun", "install", "--frozen-lockfile"], cwd=excerpt)
        run(["bun", "run", "check"], cwd=excerpt)
    distribution = root / "dist"
    with measured("consumer_build", "app_build"):
        build_kwargs = (
            {"env": package_build_environment(root / "host-build-home")}
            if args.package_only else {}
        )
        run([cli, "app", "build", "--root", str(project), "--out", str(distribution)], **build_kwargs)
    runtime_environment = {
        "PATH": str(root / "no-tools"),
        "LENSO_REFERENCE_DATABASE_URL": database_url,
        "LENSO_AUTH_SIGNING_SECRET": signing_secret,
        "LENSO_AUTH_TOKEN_PEPPER": token_pepper,
    }
    unadopt_receipt = None
    if args.package_only:
        with measured("consumer_build", "app_check_show"):
            intent = distribution / "intent"
            check_environment = package_build_environment(root / "host-build-home")
            run([cli, "app", "check", "--root", str(intent)], env=check_environment)
            shown = json.loads(subprocess.run(
                [cli, "app", "show", "--root", str(intent), "--json"],
                check=True, capture_output=True, text=True, env=check_environment,
            ).stdout)
            instance_ids = {instance["id"] for instance in shown["instances"]}
            for plugin_id in PLUGIN_IDS.values():
                if f"{plugin_id}/default" not in instance_ids:
                    raise RuntimeError(f"{plugin_id} is missing from the built App")
        with measured("consumer_build", "linked_unadopt_check_show"):
            jobs_coordinate, jobs_archive = releases["jobs"]
            jobs_version = jobs_coordinate.rsplit("@", 1)[1]
            with tempfile.TemporaryDirectory(prefix="unadopt-probe-", dir=root) as probe_name:
                probe = Path(probe_name) / "app"
                shutil.copytree(
                    project, probe,
                    ignore=shutil.ignore_patterns("host-build.json", "host-catalog.json"),
                )
                probe_host_authority = probe / ".lenso" / "host-build.json"
                probe_host_authority.parent.mkdir(exist_ok=True)
                shutil.copyfile(distribution / ".lenso" / "host-build.json", probe_host_authority)
                _, jobs_lock = adopted_operator_source(probe, "jobs", jobs_version, jobs_archive)
                jobs_lock_record = json.loads(jobs_lock)
                check_environment = package_build_environment(root / "host-build-home")
                run([
                    cli, "plugins", "bind", "--root", str(probe),
                    "lenso.reference.knowledge-excerpt", "jobs", "--absent",
                ], env=check_environment)
                probe_host_authority.unlink()
                # A customized Instance is user-owned; only the disposable probe
                # restores app add's generated intent before source unadoption.
                (probe / "plugins" / PLUGIN_IDS["jobs"] / "default.toml").write_text(
                    "# Explicit local Plugin adoption\n"
                )
                run([
                    cli, "app", "unadopt", "--root", str(probe), jobs_coordinate,
                ], env=check_environment)
                if (probe / "vendor" / "lenso" / PLUGIN_IDS["jobs"] / jobs_version).exists():
                    raise RuntimeError("Jobs linked source remains after exact unadopt")
                if (probe / "plugins" / PLUGIN_IDS["jobs"]).exists():
                    raise RuntimeError("Jobs Plugin Root intent remains after exact unadopt")
                with (probe / "lenso.toml").open("rb") as manifest:
                    remaining_sources = tomllib.load(manifest).get("plugin_sources", [])
                if f"vendor/lenso/{PLUGIN_IDS['jobs']}/{jobs_version}" in remaining_sources:
                    raise RuntimeError("Jobs linked source remains selected after unadopt")
                for name in ("auth", "secrets"):
                    version = releases[name][0].rsplit("@", 1)[1]
                    if f"vendor/lenso/{PLUGIN_IDS[name]}/{version}" not in remaining_sources:
                        raise RuntimeError(f"{name} source was removed with Jobs")
                trash_sources = list(
                    (probe / ".lenso" / "trash" / "linked-cargo").glob("unadopt-*/source")
                )
                if len(trash_sources) != 1:
                    raise RuntimeError("Jobs unadopt did not leave one recoverable source")
                if (trash_sources[0] / ".lenso-linked-source.json").read_bytes() != jobs_lock:
                    raise RuntimeError("Jobs source lock changed during unadopt")
                if linked_source_digest(trash_sources[0]) != jobs_lock_record["source_digest"]:
                    raise RuntimeError("Jobs recoverable source differs from signed adoption")
                if probe_host_authority.exists():
                    raise RuntimeError("unadopted App build would reuse prior Host authority")
                removed_distribution = root / "dist-unadopted"
                run([
                    cli, "app", "build", "--root", str(probe),
                    "--out", str(removed_distribution),
                ], env=check_environment)
                removed_intent = removed_distribution / "intent"
                run([cli, "app", "check", "--root", str(removed_intent)], env=check_environment)
                shown = json.loads(subprocess.run(
                    [cli, "app", "show", "--root", str(removed_intent), "--json"],
                    check=True, capture_output=True, text=True, env=check_environment,
                ).stdout)
                if shown["kind"] != "lenso.app-show":
                    raise RuntimeError("unexpected rebuilt App show response after Jobs unadopt")
                remaining_instances = {instance["id"] for instance in shown["instances"]}
                if f"{PLUGIN_IDS['jobs']}/default" in remaining_instances:
                    raise RuntimeError("Jobs Instance remains selected after unadopted App build")
                for name in ("auth", "secrets"):
                    if f"{PLUGIN_IDS[name]}/default" not in remaining_instances:
                        raise RuntimeError(f"{name} Instance disappeared from unadopted App build")
                tokens = issue_tokens()
                process, reader, transcript, url = launch(
                    cli, removed_distribution, root, runtime_environment
                )
                try:
                    removed_note = http_json(
                        url.rstrip("/") + "/notes", method="POST", expected=201,
                        token=tokens["user-a"],
                        body={
                            "title": "Unadopted Jobs",
                            "body": "The rebuilt App processes without Jobs.",
                        },
                    )
                    if (removed_note["processing_status"] != "succeeded"
                            or removed_note["job_id"] != "inline:" + removed_note["id"]):
                        raise RuntimeError("unadopted App did not process without Jobs")
                finally:
                    stop(process, reader, transcript)
                unadopt_receipt = {
                    "plugin_id": PLUGIN_IDS["jobs"],
                    "version": jobs_version,
                    "crate_digest": jobs_lock_record["crate_digest"],
                    "source_digest": jobs_lock_record["source_digest"],
                    "source_and_intent_removed": True,
                    "recoverable_source_in_trash": True,
                    "rebuilt_app_check_show": True,
                    "rebuilt_app_processed_without_jobs": True,
                }
    with measured("plugin_candidate_setup", "runtime_credentials"):
        tokens = issue_tokens()
    distribution_bytes = tree_logical_bytes(distribution)
    lifecycle_root = root / "runtime-app"
    (lifecycle_root / ".lenso").mkdir(parents=True)
    host_authority = lifecycle_root / ".lenso" / "host-build.json"
    shutil.copyfile(
        distribution / ".lenso" / "host-build.json",
        host_authority,
    )
    shutil.copytree(project / "plugins", lifecycle_root / "plugins")
    jobs_configuration = lifecycle_root / "plugins" / "lenso.jobs" / "default.toml"
    jobs_configuration_text = jobs_configuration.read_text()
    run([
        cli, "plugins", "bind", "--root", str(lifecycle_root),
        "lenso.reference.knowledge-excerpt", "jobs", "--absent",
    ])
    run([cli, "plugins", "disable", "--root", str(lifecycle_root), "lenso.jobs", "default"])
    assert jobs_configuration.read_text() == jobs_configuration_text
    host_authority.unlink()
    shutil.rmtree(source)
    process, reader, transcript, url = launch(cli, distribution, root, runtime_environment)
    created = None
    try:
        with urllib.request.urlopen(url, timeout=10) as response:
            home = response.read().decode()
            assert response.status == 200
            assert '<div id="root"></div>' in home
        with urllib.request.urlopen(url.rstrip("/") + "/assets/app.js", timeout=10) as response:
            assert response.status == 200
            assert "Knowledge base" in response.read().decode()

        expect_http_error(
            url.rstrip("/") + "/notes", 401, method="POST",
            body={"title": "Denied", "body": "No credential"},
        )
        settings = http_json(url.rstrip("/") + "/settings", token=tokens["user-a"])
        assert settings == {"excerpt_limit": 96, "revision": 1}
        settings = http_json(
            url.rstrip("/") + "/settings", method="PUT", token=tokens["user-a"],
            body={"excerpt_limit": 48, "predecessor_revision": 1},
        )
        assert settings == {"excerpt_limit": 48, "revision": 2}
        expect_http_error(
            url.rstrip("/") + "/settings", 409, method="PUT", token=tokens["user-a"],
            body={"excerpt_limit": 64, "predecessor_revision": 1},
        )

        note_body = (
            "Created from the offline distribution through a real TypeScript Plugin "
            "without a model credential or source checkout at runtime."
        )
        created = http_json(
            url.rstrip("/") + "/notes", method="POST", expected=201, token=tokens["user-a"],
            body={"title": "First note", "body": note_body},
        )
        assert created["id"].startswith("note-")
        assert created["excerpt"] == ""
        assert created["job_id"].startswith("job_")
        assert created["processing_status"] == "queued"
        processed = http_json(
            url.rstrip("/") + "/jobs/process-next", method="POST", body={}, token=tokens["user-a"]
        )
        assert processed == {"processed": True}
        created = http_json(
            url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]
        )
        assert created["excerpt"] == note_body[:47] + "…"
        assert created["processing_status"] == "succeeded"
        durable = http_json(
            url.rstrip("/") + "/job-status/" + created["job_id"], token=tokens["user-a"]
        )
        assert durable == {"attempts": 1, "jobId": created["job_id"], "status": "succeeded"}

        attachment = http_json(
            url.rstrip("/") + f"/note-attachments/{created['id']}",
            method="POST", expected=201, token=tokens["user-a"],
            body={
                "content_base64": base64.b64encode(b"reference attachment").decode(),
                "filename": "reference.txt",
                "media_type": "text/plain",
            },
        )
        assert attachment["note_id"] == created["id"]
        assert attachment["size"] == len(b"reference attachment")
        expect_http_error(
            url.rstrip("/") + f"/note-attachments/{created['id']}", 404,
            method="POST", token=tokens["user-b"],
            body={
                "content_base64": base64.b64encode(b"not my note").decode(),
                "filename": "denied.txt",
                "media_type": "text/plain",
            },
        )
        expect_http_error(
            url.rstrip("/") + f"/note-attachments/{created['id']}", 400,
            method="POST", token=tokens["user-a"],
            body={"content_base64": "!!!", "filename": "invalid.txt", "media_type": "text/plain"},
        )
        expect_http_error(
            url.rstrip("/") + "/notes/" + created["id"], 404, token=tokens["user-b"]
        )
        expect_http_error(
            url.rstrip("/") + "/job-status/" + created["job_id"], 404, token=tokens["user-b"]
        )
        if args.browser_handoff:
            handoff = args.browser_handoff.resolve()
            handoff.write_text(json.dumps({"token": tokens["user-a"], "url": url}))
            handoff.chmod(0o600)
            deadline = time.monotonic() + 600
            while handoff.exists() and time.monotonic() < deadline:
                time.sleep(0.25)
            if handoff.exists():
                raise TimeoutError(f"browser handoff was not removed: {handoff}")
    finally:
        stop(process, reader, transcript)

    process, reader, transcript, url = launch(
        cli, distribution, root, runtime_environment, app_root=lifecycle_root
    )
    try:
        preserved = http_json(
            url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]
        )
        assert preserved == created
        inline_body = "Jobs is disabled, so deterministic processing completes inline."
        inline = http_json(
            url.rstrip("/") + "/notes", method="POST", expected=201, token=tokens["user-a"],
            body={"title": "Disabled Jobs", "body": inline_body},
        )
        assert inline["excerpt"] == inline_body[:47] + "…"
        assert inline["job_id"] == "inline:" + inline["id"]
        assert inline["processing_status"] == "succeeded"
    finally:
        stop(process, reader, transcript)

    shutil.copyfile(distribution / ".lenso" / "host-build.json", host_authority)
    run([cli, "plugins", "enable", "--root", str(lifecycle_root), "lenso.jobs", "default"])
    run([
        cli, "plugins", "bind", "--root", str(lifecycle_root),
        "lenso.reference.knowledge-excerpt", "jobs", "lenso.jobs",
    ])
    assert jobs_configuration.read_text() == jobs_configuration_text
    host_authority.unlink()
    process, reader, transcript, url = launch(
        cli, distribution, root, runtime_environment, app_root=lifecycle_root
    )
    try:
        restarted = http_json(
            url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]
        )
        assert restarted == created
        durable = http_json(
            url.rstrip("/") + "/job-status/" + created["job_id"], token=tokens["user-a"]
        )
        assert durable["status"] == "succeeded"
        assert durable["attempts"] == 1
    finally:
        stop(process, reader, transcript)

    # Remove the root-supplied Jobs Plugin, not merely its disabled marker.
    # The CLI validates the candidate App before moving the Plugin Root to
    # recoverable trash. The immutable Host distribution and business data stay.
    shutil.copyfile(distribution / ".lenso" / "host-build.json", host_authority)
    run([
        cli, "plugins", "bind", "--root", str(lifecycle_root),
        "lenso.reference.knowledge-excerpt", "jobs", "--absent",
    ])
    run([cli, "plugins", "remove", "--root", str(lifecycle_root), "lenso.jobs"])
    assert not (lifecycle_root / "plugins" / "lenso.jobs").exists()
    removed_roots = list((lifecycle_root / ".lenso" / "trash").glob("lenso.jobs-*"))
    assert len(removed_roots) == 1
    assert (removed_roots[0] / "default.toml").read_text() == jobs_configuration_text
    run([cli, "app", "check", "--root", str(lifecycle_root)])
    shown = json.loads(subprocess.run(
        [cli, "app", "show", "--root", str(lifecycle_root), "--json"],
        check=True, capture_output=True, text=True,
    ).stdout)
    assert shown["kind"] == "lenso.app-show"
    assert all(not instance["id"].startswith("lenso.jobs/") for instance in shown["instances"])
    host_authority.unlink()
    process, reader, transcript, url = launch(
        cli, distribution, root, runtime_environment, app_root=lifecycle_root
    )
    try:
        assert http_json(url.rstrip("/") + "/notes/" + created["id"], token=tokens["user-a"]) == created
        assert http_json(url.rstrip("/") + "/settings", token=tokens["user-a"]) == settings
        expect_http_error(url.rstrip("/") + "/notes/" + created["id"], 404, token=tokens["user-b"])
        removed_jobs_body = "The persisted workspace survives removal of its optional Jobs Plugin."
        without_jobs = http_json(
            url.rstrip("/") + "/notes", method="POST", expected=201, token=tokens["user-a"],
            body={"title": "Removed Jobs", "body": removed_jobs_body},
        )
        assert without_jobs["job_id"] == "inline:" + without_jobs["id"]
        assert without_jobs["processing_status"] == "succeeded"
        assert without_jobs["excerpt"] == removed_jobs_body[:47] + "…"
    finally:
        stop(process, reader, transcript)

print("MEASUREMENT " + json.dumps({
    "kind": "lenso.reference-build-measurement",
    "environment": {
        "os": platform.system(),
        "machine": platform.machine(),
        "python_version": platform.python_version(),
        "cli_version": command_version([cli, "--version"]),
        "cargo_version": command_version(["cargo", "--version"]),
        "bun_version": command_version(["bun", "--version"]),
    },
    "cache_state": "ambient Cargo and Bun caches; not a controlled cold or warm build",
    "provider_input_mode": "signed_crate_derived_operators" if args.package_only else "source_checkout",
    "operator_receipts": operator_receipts,
    "unadopt_receipt": unadopt_receipt,
    "disk_scope": "logical regular-file bytes in the temporary consumer tree; external caches and databases excluded",
    "distribution_bytes": distribution_bytes,
    "phases": phases,
}, sort_keys=True))
print(
    "PASS: "
    + ("signed-catalog .crate provider adoption with crate-derived operators, " if args.package_only else "")
    + "source-deleted React, Auth isolation, PostgreSQL notes/files/settings, "
    "Rust-to-TypeScript durable Jobs, disable/enable/remove preservation, and restart"
)
