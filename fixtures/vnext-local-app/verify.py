#!/usr/bin/env python3
"""Build and verify the mixed local App using an implementation-capable CLI."""
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
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--cli', default='lenso')
args = parser.parse_args()
cli = str(Path(shutil.which(args.cli) or args.cli).resolve())
fixture = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix='lenso-local-example-') as temporary:
    root = Path(temporary)
    source = root / 'source'
    shutil.copytree(fixture, source, ignore=shutil.ignore_patterns(
        'node_modules', 'target', '.lenso', 'dist', '__pycache__'))
    subprocess.run(['bun', 'install', '--frozen-lockfile'],
                   cwd=source / 'project/app/uppercase', check=True)
    distribution = root / 'dist'
    subprocess.run([cli, 'app', 'build', '--root', str(source / 'project'),
                    '--out', str(distribution)], check=True)
    # Production must not consult any of the source directories or toolchains.
    shutil.rmtree(source)
    process = subprocess.Popen(
        [cli, 'app', 'start', '--from', str(distribution)],
        env={'PATH': str(root / 'no-tools')}, cwd=root,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        start_new_session=True)
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
                raise RuntimeError('Host exited before readiness')
            match = re.search(r'Listening on (http://\S+)', line)
            if match:
                url = match[1]
                break
        assert url, 'Host did not report Web readiness'
        with urllib.request.urlopen(url, timeout=10) as response:
            assert '<html' in response.read().decode().lower()
        request = urllib.request.Request(
            url.rstrip('/') + '/uppercase',
            data=json.dumps({'text': 'native web to bun'}).encode(),
            headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=10) as response:
            assert json.load(response) == {'text': 'NATIVE WEB TO BUN'}
        assert any('Shared audit Plugin activated by explicit Root intent' in line
                   for line in transcript)
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
        print(''.join(transcript))
    assert process.returncode == 0
    assert any('Shared audit Plugin stopped' in line for line in transcript)
    assert any('Local App stopped cleanly' in line for line in transcript)
print('PASS: offline HTTP -> native Rust -> Bun, explicit shared adoption, shutdown')
