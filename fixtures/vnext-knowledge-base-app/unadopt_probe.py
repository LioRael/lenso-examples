"""Copy a disposable App while retaining its installed package entry points."""

import shutil


def copy_unadopt_probe(project, probe):
    shutil.copytree(
        project,
        probe,
        symlinks=True,
        ignore=shutil.ignore_patterns("host-build.json", "host-catalog.json"),
    )
