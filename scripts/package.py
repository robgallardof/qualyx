"""Package a built binary with licenses, checksum it and run an extracted-install smoke."""
import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

REPO = Path(__file__).resolve().parents[1]


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
        for file in ["LICENSE-MIT", "LICENSE-APACHE", "README.md", "README.es.md"]:
            shutil.copy2(REPO / file, package / file)
        archive = Path(shutil.make_archive(str(args.out / name), "zip" if os.name == "nt" else "gztar", root_dir=root, base_dir=name))
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(archive.name + ".sha256").write_text(checksum + "  " + archive.name + "\n")
        installed = root / "installed"
        shutil.unpack_archive(archive, installed)
        executable = installed / name / binary.name
        subprocess.run([str(executable), "--version"], check=True)
        for fixture, expected in [("passing", 0), ("failing", 1)]:
            project = root / fixture
            shutil.copytree(REPO / "fixtures/python" / fixture, project)
            subprocess.run([str(executable), "doctor", str(project)], check=True, capture_output=True)
            result = subprocess.run([str(executable), "test", str(project)], capture_output=True, text=True)
            assert result.returncode == expected, (fixture, result.stdout, result.stderr)
        print("PASS: extracted installation, doctor, seeded passing/failing fixtures, SHA256: " + archive.name)


if __name__ == "__main__":
    main()
