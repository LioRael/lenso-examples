"""The package-only unadopt probe must preserve Bun executable symlinks."""

import tempfile
import unittest
from pathlib import Path

from unadopt_probe import copy_unadopt_probe


class UnadoptProbeTests(unittest.TestCase):
    def test_copied_bun_entry_point_still_targets_its_package(self):
        with tempfile.TemporaryDirectory(prefix="lenso-unadopt-probe-") as temporary:
            root = Path(temporary)
            project = root / "project"
            package = project / "app" / "excerpt" / "node_modules" / "typescript"
            (package / "bin").mkdir(parents=True)
            (package / "bin" / "tsc").write_text("entry point")
            binaries = project / "app" / "excerpt" / "node_modules" / ".bin"
            binaries.mkdir()
            (binaries / "tsc").symlink_to("../typescript/bin/tsc")
            authority = project / ".lenso"
            authority.mkdir()
            (authority / "host-build.json").write_text("old host")

            probe = root / "probe"
            copy_unadopt_probe(project, probe)

            copied = probe / "app" / "excerpt" / "node_modules" / ".bin" / "tsc"
            self.assertTrue(copied.is_symlink())
            self.assertEqual(copied.readlink(), Path("../typescript/bin/tsc"))
            self.assertEqual(copied.resolve().read_text(), "entry point")
            self.assertFalse((probe / ".lenso" / "host-build.json").exists())


if __name__ == "__main__":
    unittest.main()
