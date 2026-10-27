"""Exercise the shipped Compose deployment, including recreation and S3-backed reads."""
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
from urllib.parse import urlencode
from test_team import REPO, TOKEN, request, await_state


def main():
    with tempfile.TemporaryDirectory(prefix="qualyx-compose-") as temp:
        root = Path(temp).resolve()
        for name in ["passing", "failing"]:
            shutil.copytree(REPO / "fixtures/python" / name, root / name)
        # Only these disposable fixture copies are writable by the non-root image user.
        for path in [root, *root.rglob("*")]:
            path.chmod(0o777 if path.is_dir() else 0o666)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        env = os.environ.copy()
        env.update(QUALYX_PROJECTS_DIR=root.as_posix(), QUALYX_API_PORT=str(port), QUALYX_API_TOKEN=TOKEN,
                   QUALYX_POSTGRES_PASSWORD=secrets.token_hex(24), QUALYX_S3_ACCESS_KEY="qualyx",
                   QUALYX_S3_SECRET_KEY=secrets.token_hex(24))
        compose = ["docker", "compose", "-p", "qualyx-acceptance-" + str(os.getpid()), "-f", str(REPO / "deploy/team/compose.yaml")]
        base = f"http://127.0.0.1:{port}"

        def run(*args):
            return subprocess.run([*compose, *args], env=env, capture_output=True, text=True, encoding="utf-8", errors="replace", check=True).stdout.strip()

        def as_worker(code):
            return run("run", "--rm", "--no-deps", "--entrypoint", "python", "worker", "-c", code)

        try:
            run("up", "-d", "--no-build", "--wait", "--wait-timeout", "90")
            assert run("exec", "-T", "worker", "id", "-u") == "10001", "non-root worker"
            assert "QUALYX_API_TOKEN" not in run("exec", "-T", "worker", "env"), "worker never receives API credential"
            run("stop", "worker")
            job = request(base, "/v1/runs", {"project": "/projects/passing"}, 202)["run_id"]
            assert request(base, "/v1/runs/" + job)["state"] == "queued"
            run("up", "-d", "--force-recreate", "--no-deps", "--wait", "api")
            assert request(base, "/v1/runs/" + job)["state"] == "queued"
            run("start", "worker")
            result = await_state(base, job, {"finished", "failed"}, timeout=90)
            assert result["state"] == "finished" and result["manifest"]["status"] == "passed", result
            failed = request(base, "/v1/runs", {"project": "/projects/failing"}, 202)["run_id"]
            result = await_state(base, failed, {"finished", "failed"}, timeout=90)
            assert result["manifest"]["status"] == "failed", result
            # Recreate every service while retaining the named database/object volumes.
            run("down", "--timeout", "30")
            as_worker("from pathlib import Path; Path('/projects/passing/.qualyx/runs/" + job + "').rename('/projects/saved-evidence')")
            run("up", "-d", "--no-build", "--wait", "--wait-timeout", "90")
            assert request(base, "/v1/runs/" + job)["manifest"]["status"] == "passed"
            xml = request(base, f"/v1/runs/{job}/artifacts/pytest.junit.xml?" + urlencode({"project": "/projects/passing"}))
            assert "test_multiplies_two_numbers" in xml, "persisted S3 evidence after full recreation"
            print("PASS: shipped Compose, non-root worker, credential separation, queued API recreation, passing/failing fixtures, database/object-volume survival and S3 restore")
        except BaseException:
            print(run("logs", "--tail", "80"))
            raise
        finally:
            try:
                # Linux bind-mount files belong to UID 10001. Make only worker-owned
                # disposable evidence removable before the host tempdir cleans up.
                run("stop", "worker")
                as_worker("import os; from pathlib import Path; "
                          "[(p.chmod(0o777 if p.is_dir() else 0o666)) for p in Path('/projects').rglob('*') "
                          "if not p.is_symlink() and p.stat().st_uid == os.getuid()]")
            finally:
                run("down", "--volumes", "--remove-orphans", "--timeout", "30")


if __name__ == "__main__":
    main()
