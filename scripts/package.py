"""Package a built binary with licenses, checksum it and run an extracted-install smoke."""
import argparse
import hashlib
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
from urllib.parse import unquote, urlsplit

REPO = Path(__file__).resolve().parents[1]


def stage_documentation(package):
    """Ship tracked guides/assets/skills/SDK without caches or local credentials."""
    files = subprocess.run(["git", "ls-files", "-z"], cwd=REPO, check=True,
                           capture_output=True).stdout.decode().split("\0")
    roots = {"LICENSE-MIT", "LICENSE-APACHE", "README.md", "README.es.md", "SECURITY.md",
             "AGENTS.md", "CONTRIBUTING.md", "readme.json", ".readme-builder-manifest.json"}
    prefixes = ("docs/", "assets/", ".agents/skills/", "sdk/python/", ".github/workflows/")
    for name in files:
        if name in roots or name.startswith(prefixes):
            destination = package / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, destination)
    shutil.copy2(REPO / "INSTALL.md", package / "INSTALL.md")


def validate_documentation(package):
    """Check the extracted reader-facing Markdown and embedded local assets."""
    root = package.resolve()
    for page in root.rglob("*.md"):
        # These are fragments authored relative to the generated root README.
        if "docs/readme/content/" in page.relative_to(root).as_posix():
            continue
        text = page.read_text(encoding="utf-8")
        links = re.findall(r'\]\(([^\s)]+)\)', text)
        links += re.findall(r'(?:src|srcset|href)="([^"]+)"', text)
        for link in links:
            url = urlsplit(link.strip("<>"))
            if url.scheme or url.netloc or not url.path:
                continue
            target = (page.parent / unquote(url.path)).resolve()
            if not target.is_relative_to(root) or not target.exists():
                raise ValueError(f"broken packaged link: {page.relative_to(root)} -> {link}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--target", default=platform.system().lower() + "-" + platform.machine().lower())
    parser.add_argument("--out", type=Path, default=REPO / "artifacts/release")
    args = parser.parse_args()
    binary = args.binary.resolve()
    version = subprocess.run([str(binary), "--version"], check=True, capture_output=True, text=True).stdout.strip().split()[-1]
    args.out.mkdir(parents=True, exist_ok=True)
    name = "qualyx-" + version + "-" + args.target
    with tempfile.TemporaryDirectory(prefix="qualyx-package-") as temp:
        root = Path(temp)
        package = root / name
        package.mkdir()
        shutil.copy2(binary, package / binary.name)
        stage_documentation(package)
        archive = Path(shutil.make_archive(str(args.out / name), "zip" if os.name == "nt" else "gztar", root_dir=root, base_dir=name))
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(archive.name + ".sha256").write_text(checksum + "  " + archive.name + "\n")
        installed = root / "installed"
        shutil.unpack_archive(archive, installed)
        executable = installed / name / binary.name
        validate_documentation(installed / name)
        if not (installed / name / ".agents/skills/qualyx-security-audit/SKILL.md").is_file():
            raise ValueError("security audit skill missing from extracted package")
        subprocess.run([str(executable), "--version"], check=True)
        for fixture, expected in [("passing", 0), ("failing", 1)]:
            project = root / fixture
            shutil.copytree(REPO / "fixtures/python" / fixture, project)
            subprocess.run([str(executable), "doctor", str(project)], check=True, capture_output=True)
            result = subprocess.run([str(executable), "test", str(project)], capture_output=True, text=True)
            assert result.returncode == expected, (fixture, result.stdout, result.stderr)
        print("PASS: extracted installation, documentation/assets/skill, doctor, seeded passing/failing fixtures, SHA256: " + archive.name)


if __name__ == "__main__":
    main()
