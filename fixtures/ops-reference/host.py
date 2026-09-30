"""Lifecycle checks for a task-owned offline Native Host process."""
from contextlib import contextmanager
from pathlib import Path
import queue
import re
import signal
import subprocess
import threading
import time


def rejected_native_start(cli, distribution, facilities, directory, expected_error):
    process = subprocess.Popen([str(cli), 'app', 'start', '--from', str(distribution),
        '--host-facilities', str(facilities)], cwd=directory,
        env={'PATH': str(Path(directory) / 'no-toolchains')},
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        output, _ = process.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=15)
        raise AssertionError('The rejected Host remained running') from None
    assert process.returncode != 0, 'An invalid authority resource was admitted'
    assert 'Listening on ' not in output and 'Local App ready' not in output, 'The rejected Host opened its surface'
    assert expected_error in output, 'The Host failed for an unrelated reason'


@contextmanager
def native_server(cli, distribution, facilities, directory, ready_marker=None):
    arguments = [str(cli), 'app', 'start', '--from', str(distribution)]
    if facilities is not None:
        arguments.extend(['--host-facilities', str(facilities)])
    process = subprocess.Popen(arguments,
                               env={'PATH': str(Path(directory) / 'no-toolchains')}, cwd=directory,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    output, transcript = queue.Queue(), []

    def read():
        for line in process.stdout:
            transcript.append(line)
            output.put(line)
        output.put(None)

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    try:
        deadline = time.monotonic() + 30
        url = None
        marked = ready_marker is None
        while time.monotonic() < deadline:
            line = output.get(timeout=max(0.1, deadline - time.monotonic()))
            if line is None:
                raise RuntimeError('Host exited before readiness: ' + ''.join(transcript))
            matched = re.search(r'Listening on (http://\S+)', line)
            if matched:
                url = matched[1]
            if ready_marker is not None and ready_marker in line:
                marked = True
            if url and marked:
                yield url
                break
        else:
            raise RuntimeError('Host never reported readiness')
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
        process.wait(timeout=15)
        reader.join(timeout=2)
        assert process.returncode == 0, ''.join(transcript)
        assert any('Local App stopped cleanly' in line for line in transcript), transcript
