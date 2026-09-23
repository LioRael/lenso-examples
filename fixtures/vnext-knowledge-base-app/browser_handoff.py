"""A local, one-use credential handoff for the reference App's browser check."""

import json
import os
import stat
import time
from contextlib import contextmanager
from pathlib import Path


MAX_PATH_BYTES = 4096
MAX_NAME_BYTES = 255
MAX_PAYLOAD_BYTES = 4096


def _directory_flags():
    if os.name != "posix" or not all(
        hasattr(os, flag) for flag in ("O_DIRECTORY", "O_NOFOLLOW", "O_CLOEXEC")
    ):
        raise RuntimeError("browser handoff requires POSIX no-follow directory opens")
    return os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC


def _open_parent(path):
    requested = Path(path)
    name = requested.name
    if name in ("", ".", "..") or len(os.fsencode(name)) > MAX_NAME_BYTES:
        raise ValueError("browser handoff requires a short, ordinary filename")
    parent = requested.parent.resolve(strict=True)
    handoff = parent / name
    if len(os.fsencode(handoff)) > MAX_PATH_BYTES:
        raise ValueError("browser handoff path is too long")

    flags = _directory_flags()
    descriptor = os.open(parent.anchor, flags)
    try:
        root_metadata = os.fstat(descriptor)
        if root_metadata.st_mode & 0o022 and not root_metadata.st_mode & stat.S_ISVTX:
            raise ValueError("browser handoff root directory is writable without a sticky bit")
        for component in parent.parts[1:]:
            next_descriptor = os.open(component, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = next_descriptor
            metadata = os.fstat(descriptor)
            if metadata.st_mode & 0o022 and not metadata.st_mode & stat.S_ISVTX:
                raise ValueError(
                    f"browser handoff directory is writable without a sticky bit: {parent}"
                )
        return handoff, descriptor
    except BaseException:
        os.close(descriptor)
        raise


class BrowserHandoff:
    def __init__(self, path, parent_fd, file_fd):
        self.path = path
        self.parent_fd = parent_fd
        self.file_fd = file_fd
        metadata = os.fstat(file_fd)
        self.identity = (metadata.st_dev, metadata.st_ino)

    def still_present(self):
        try:
            metadata = os.stat(
                self.path.name, dir_fd=self.parent_fd, follow_symlinks=False
            )
        except FileNotFoundError:
            return False
        if (metadata.st_dev, metadata.st_ino) != self.identity or not stat.S_ISREG(
            metadata.st_mode
        ):
            raise RuntimeError(
                "browser handoff path was replaced; refusing to remove it. "
                f"The original credential may remain under another name: {self.path}"
            )
        return True

    def wait_for_removal(self, timeout=600, poll_interval=0.25):
        deadline = time.monotonic() + timeout
        while self.still_present():
            if time.monotonic() >= deadline:
                raise TimeoutError(
                    f"browser handoff was not removed within {timeout} seconds; "
                    f"its credential file will be removed: {self.path}"
                )
            time.sleep(poll_interval)

    def close(self):
        try:
            if self.still_present():
                os.unlink(self.path.name, dir_fd=self.parent_fd)
                os.fsync(self.parent_fd)
        finally:
            os.close(self.file_fd)
            os.close(self.parent_fd)


@contextmanager
def browser_handoff(path, token, url):
    payload = json.dumps({"token": token, "url": url}).encode("utf-8")
    if len(payload) > MAX_PAYLOAD_BYTES:
        raise ValueError("browser handoff payload is too large")
    handoff_path, parent_fd = _open_parent(path)
    file_fd = None
    try:
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC
        file_fd = os.open(handoff_path.name, flags, 0o600, dir_fd=parent_fd)
        os.fchmod(file_fd, 0o600)
        metadata = os.fstat(file_fd)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise RuntimeError("browser handoff was not created as a single regular file")
        remaining = memoryview(payload)
        while remaining:
            written = os.write(file_fd, remaining)
            if written <= 0:
                raise OSError("browser handoff write did not make progress")
            remaining = remaining[written:]
        os.fsync(file_fd)
        os.fsync(parent_fd)
        handoff = BrowserHandoff(handoff_path, parent_fd, file_fd)
    except BaseException:
        if file_fd is not None:
            created = BrowserHandoff(handoff_path, parent_fd, file_fd)
            created.close()
        else:
            os.close(parent_fd)
        raise
    try:
        yield handoff
    finally:
        handoff.close()
