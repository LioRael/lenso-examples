#!/usr/bin/env python3
"""Own a disposable loopback PostgreSQL cluster and reap every child on failure."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import secrets
import selectors
import socket
import subprocess
import tempfile
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent


def run(*command, **kwargs):
    try:
        return subprocess.run(command, check=True, text=True, **kwargs)
    except subprocess.CalledProcessError as error:
        if error.stderr:
            print(error.stderr)
        raise


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def stop(process):
    if process is not None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()

def start_service(binary, env):
    process = subprocess.Popen([str(ROOT / ".target/debug" / binary)], env=env,
                               stdout=subprocess.PIPE, text=True)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            if not selector.select(timeout=10):
                raise RuntimeError(f"{binary} startup timed out")
            origin = process.stdout.readline().strip()
        if not origin.startswith("http://127.0.0.1:"):
            raise RuntimeError(f"{binary} did not report a loopback listener")
        return process, origin
    except BaseException:
        stop(process)
        raise


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--workers", action="store_true")
    args = parser.parse_args()
    # Compile and run all pure tests before starting a database.
    run("python3", str(ROOT / "verify.py"), "build")
    with tempfile.TemporaryDirectory(prefix="portable-settings-pg-") as temporary:
        directory = Path(temporary)
        data = directory / "data"
        run("initdb", "-D", str(data), "-A", "trust", "-U", "fixture",
            "--no-locale", stdout=subprocess.DEVNULL)
        port = free_port()
        bridge = None
        native_process = None
        peer_process = None
        started = False
        env = dict(os.environ, SETTINGS_DATABASE_URL=f"host=127.0.0.1 port={port} user=fixture dbname=postgres",
                   SETTINGS_BRIDGE_TOKEN=secrets.token_hex(32))
        try:
            run("pg_ctl", "-D", str(data), "-l", str(directory / "postgres.log"),
                "-o", f"-h 127.0.0.1 -p {port} -k ''", "-w", "start",
                stdout=subprocess.DEVNULL)
            started = True
            run(str(ROOT / ".target/debug/native"), "--initialize", env=env)
            native_process, native_origin = start_service("native", env)

            def native(method="GET", body=None, principal="shared", uri="/settings",
                       content_type="application/json", origin=None):
                request = urllib.request.Request(
                    (origin or native_origin) + uri, method=method,
                    headers={"x-local-test-principal": principal, "content-type": content_type},
                    data=None if body is None else json.dumps(body).encode())
                try:
                    response = urllib.request.urlopen(request, timeout=10)
                except urllib.error.HTTPError as error:
                    response = error
                with response:
                    content = response.read().decode()
                    try:
                        content = json.loads(content)
                    except json.JSONDecodeError:
                        pass
                    return {"status": response.status, "body": content}

            assert native() == {"status": 200, "body": {"revision": 0, "value": ""}}
            assert native(uri="/not-settings")["status"] == 404
            assert native("PUT", {})["status"] == 400
            assert native("PUT", {}, content_type="text/plain")["status"] == 415
            change = {"expected_revision": 0, "idempotency_key": "native", "value": "native"}
            written = {"status": 200, "body": {"revision": 1, "value": "native"}}
            # Separate HTTP Hosts bypass per-Instance admission serialization,
            # proving the database transaction owns concurrent idempotency.
            peer_process, peer_origin = start_service("native", env)
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(
                    lambda origin: native("PUT", change, origin=origin),
                    [native_origin, peer_origin]))
            assert results == [written, written], results
            stop(peer_process)
            assert peer_process.returncode == 0, "Native peer shutdown was not clean"
            stop(native_process)
            assert native_process.returncode == 0, "Native Kernel shutdown was not clean"
            native_process, native_origin = start_service("native", env)
            assert native() == written  # A recreated HTTP Host sees durable state.
            assert native("PUT", change) == written
            assert native("PUT", {**change, "value": "different"})["status"] == 409
            assert native("PUT", {**change, "idempotency_key": "stale"})["status"] == 409
            assert native("PUT", {**change, "value": ""})["status"] == 400
            assert native(principal="other") == {"status": 200, "body": {"revision": 0, "value": ""}}

            if args.workers:
                bridge, env["SETTINGS_BRIDGE_ORIGIN"] = start_service("bridge", env)
                try:
                    urllib.request.urlopen(urllib.request.Request(
                        env["SETTINGS_BRIDGE_ORIGIN"] + "/read", data=b"{}", method="POST"))
                    raise AssertionError("bridge accepted missing authorization")
                except urllib.error.HTTPError as error:
                    assert error.code == 401
                run("node", str(ROOT / "workerd-test.mjs"), env=env, cwd=ROOT, timeout=90)
                assert native() == {"status": 200, "body": {"revision": 2, "value": "workers"}}
                # A replay retains its first result even after the other target writes.
                assert native("PUT", change) == written
            run("pg_ctl", "-D", str(data), "-m", "fast", "-w", "stop", stdout=subprocess.DEVNULL)
            started = False
            unavailable = native()
            assert unavailable["status"] >= 500, unavailable
            if args.workers:
                run("node", str(ROOT / "workerd-test.mjs"), "--unavailable",
                    env=env, cwd=ROOT, timeout=30)
            stop(native_process)
            assert native_process.returncode == 0, "Native Kernel shutdown was not clean"
            print("native HTTP: read/CAS/concurrent replay/conflict/recreated Host/fail-closed/clean shutdown passed")
        finally:
            stop(peer_process)
            stop(native_process)
            stop(bridge)
            if started:
                run("pg_ctl", "-D", str(data), "-m", "immediate", "-w", "stop", stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
