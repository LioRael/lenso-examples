#!/usr/bin/env python3
"""Build the knowledge base App and prove its offline HTTP create/read path."""
import argparse
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--cli", default="lenso")
args = parser.parse_args()
cli = str(Path(shutil.which(args.cli) or args.cli).resolve())
fixture = Path(__file__).resolve().parent

with tempfile.TemporaryDirectory(prefix="lenso-knowledge-base-") as temporary:
    root = Path(temporary)
    source = root / "source"
    shutil.copytree(
        fixture,
        source,
        ignore=shutil.ignore_patterns("target", ".lenso", "dist", "__pycache__"),
    )
    distribution = root / "dist"
    subprocess.run(
        [cli, "app", "build", "--root", str(source / "project"), "--out", str(distribution)],
        check=True,
    )
    shutil.rmtree(source)

    process = subprocess.Popen(
        [cli, "app", "start", "--from", str(distribution)],
        env={"PATH": str(root / "no-tools")},
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
        url = None
        while time.monotonic() < deadline:
            line = events.get(timeout=max(0.1, deadline - time.monotonic()))
            if line is None:
                raise RuntimeError("Host exited before readiness")
            match = re.search(r"Listening on (http://\S+)", line)
            if match:
                url = match[1]
                break
        assert url, "Host did not report Web readiness"

        with urllib.request.urlopen(url, timeout=10) as response:
            home = response.read().decode()
            assert response.status == 200
            assert "Knowledge base" in home

        request = urllib.request.Request(
            url.rstrip("/") + "/notes",
            data=json.dumps(
                {"title": "First note", "body": "Created from the offline distribution."}
            ).encode(),
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            assert response.status == 201
            created = json.load(response)
        assert created == {
            "id": "note-1",
            "title": "First note",
            "body": "Created from the offline distribution.",
        }

        with urllib.request.urlopen(
            url.rstrip("/") + "/notes/" + created["id"], timeout=10
        ) as response:
            assert response.status == 200
            assert json.load(response) == created

        try:
            urllib.request.urlopen(url.rstrip("/") + "/notes/missing", timeout=10)
        except urllib.error.HTTPError as error:
            assert error.code == 404
        else:
            raise AssertionError("missing note must return HTTP 404")
    finally:
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

print("PASS: offline knowledge base home, create, read, rejection, shutdown")
