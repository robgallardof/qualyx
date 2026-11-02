"""Documentation checks use real files and detect incomplete extracted installations."""
from pathlib import Path
import json
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from package import stage_documentation, validate_documentation


class PackageTests(unittest.TestCase):
    def test_shipped_readmes_have_guides_images_and_security_skill(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            stage_documentation(root)
            validate_documentation(root)
            self.assertTrue((root / "INSTALL.md").is_file())
            self.assertTrue((root / ".agents/skills/qualyx-security-audit/SKILL.md").is_file())
            self.assertTrue((root / "sdk/python/qualyx_client.py").is_file())
            for name in ("Dockerfile", "compose.yaml", "mcp.example.json"):
                self.assertTrue((root / "deploy/code" / name).is_file())
            example = root / "deploy/code/mcp.example.json"
            config = json.loads(example.read_text(encoding="utf-8"))
            self.assertEqual(config["mcpServers"]["qualyx-code"]["env"]["QUALYX_CODE_MODE"], "1")
            self.assertFalse(list(root.rglob("__pycache__")))
            example.unlink()
            with self.assertRaisesRegex(ValueError, "broken packaged link"):
                validate_documentation(root)
            example.write_text(json.dumps(config), encoding="utf-8")
            (root / "assets/readme-builder/en/hero-light.svg").unlink()
            with self.assertRaisesRegex(ValueError, "broken packaged link"):
                validate_documentation(root)

    def test_reference_outside_the_package_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "README.md").write_text("[outside](../outside.md)", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "broken packaged link"):
                validate_documentation(root)


if __name__ == "__main__":
    unittest.main()
