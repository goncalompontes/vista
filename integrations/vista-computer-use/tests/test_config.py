from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from vista_computer_use.config import Config


class ConfigTests(unittest.TestCase):
    def test_explicit_environment_overrides(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "vista" / "host").mkdir(parents=True)
            (root / "vista" / "host" / "Cargo.toml").write_text("")
            host_bin = root / "bin" / "vista-host"
            host_bin.parent.mkdir()
            host_bin.write_text("")
            config = Config.from_env(
                env={
                    "VISTA_HOME": str(root / "vista"),
                    "VPHONE_HOME": str(root / "vphone"),
                    "VPHONE_DEVICES": str(root / "devices"),
                    "VPHONE_CID": "9",
                    "VPHONE_VM": "phone",
                    "VISTA_CU_STATE_DIR": str(root / "state"),
                    "VISTA_CU_SCREENSHOT_DIR": str(root / "shots"),
                    "VISTA_CU_ALLOWED_ROOTS": f"{root}:{root / 'shots'}",
                    "VISTA_HOST_BIN": str(host_bin),
                    "VISTA_CU_MAX_IMAGE_BYTES": "4096",
                    "VISTA_CU_TIMEOUT": "12",
                },
                cwd=root,
            )
            self.assertEqual(config.vphone_cid, 9)
            self.assertEqual(config.vphone_vm, "phone")
            self.assertEqual(config.vista_host_bin, host_bin)
            self.assertEqual(config.max_image_bytes, 4096)
            self.assertEqual(config.request_timeout, 12)
            self.assertIn(root.resolve(), config.allowed_write_roots)

    def test_invalid_cid_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            Config.from_env(env={"VPHONE_CID": "zero"})
        with self.assertRaises(ValueError):
            Config.from_env(env={"VPHONE_CID": "0"})

    def test_derives_workspace_from_package_location(self) -> None:
        config = Config.from_env(env={})
        self.assertTrue((config.workspace / "host" / "Cargo.toml").is_file())


if __name__ == "__main__":
    unittest.main()
