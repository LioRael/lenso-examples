"""Run preserved bridge tests with an explicitly supplied trusted generic SDK."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from prepare_integration import read_regular


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lenso-js", type=Path, required=True)
    args = parser.parse_args()
    runtime = args.lenso_js / "packages/lenso-workers-runtime"
    package = json.loads(read_regular(runtime / "package.json", 65_536))
    if (package["name"], package["version"]) != ("@lenso/workers-runtime", "0.1.5"):
        raise ValueError("explicit checkout must contain @lenso/workers-runtime 0.1.5")
    source = Path(__file__).parent
    with tempfile.TemporaryDirectory(prefix="knowledge-settings-tests-") as temporary:
        root = Path(temporary)
        for name in ("component-admission.mjs", "component-requests.mjs"):
            (root / name).write_bytes(read_regular(runtime / name, 1_048_576))
        shutil.copyfile(source / "knowledge-settings-local.mjs", root / "knowledge-settings-local.mjs")
        (root / "test").mkdir()
        shutil.copyfile(source / "test/knowledge-settings-local.test.mjs",
                        root / "test/knowledge-settings-local.test.mjs")
        subprocess.run(["node", "--test", "test/knowledge-settings-local.test.mjs"],
                       cwd=root, check=True)


if __name__ == "__main__":
    main()
