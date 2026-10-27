"""Real M7 acceptance: PostgreSQL + signed S3 + API/worker OS processes.

Uses temporary projects/containers only. No model credentials or external targets.
Run after cargo build -p qualyx-cli: python scripts/test_team.py
"""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import sys
import urllib.error
import urllib.request
from urllib.parse import urlencode

REPO = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("QUALYX_BIN", REPO / "target/debug" / ("qualyx.exe" if os.name == "nt" else "qualyx"))).resolve()
TOKEN = "local-acceptance-token-0123456789abcdef"
sys.path.insert(0, str(REPO / "sdk/python"))
from qualyx_client import QualyxClient


def command(*args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True, encoding="utf-8", errors="replace", **kwargs).stdout.strip()


def request(base, route, body=None, expected=200):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + route, data=data, headers={"Authorization": "Bearer " + TOKEN, "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            status, raw = response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        status, raw = error.code, error.read().decode()
    assert status == expected, (route, status, raw)
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return raw


def await_state(base, run, states, timeout=50):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = request(base, "/v1/runs/" + run)
        if result["state"] in states:
            return result
        time.sleep(0.15)
    raise AssertionError((run, result))


def stop(process):
    if process.poll() is None:
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True)
        else:
            import signal
            # Native runners create their own process groups. Include those descendants
            # when simulating a hard worker crash, so acceptance leaves no orphan tests.
            pairs = subprocess.run(["ps", "-eo", "pid=,ppid="], capture_output=True, text=True, check=True).stdout
            parents = [tuple(map(int, line.split())) for line in pairs.splitlines() if line.strip()]
            descendants = {process.pid}
            while True:
                expanded = descendants | {pid for pid, parent in parents if parent in descendants}
                if expanded == descendants:
                    break
                descendants = expanded
            for pid in descendants - {process.pid}:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=10)


def main():
    suffix = str(os.getpid())
    pg, s3 = "qualyx-team-pg-" + suffix, "qualyx-team-s3-" + suffix
    processes, logs = [], []
    with tempfile.TemporaryDirectory(prefix="qualyx-team-") as temp:
        root = Path(temp).resolve()
        env = os.environ.copy()
        try:
            command("docker", "run", "-d", "--rm", "--name", pg, "-e", "POSTGRES_PASSWORD=qualyx-local-test", "-p", "127.0.0.1::5432", "postgres:17-alpine")
            command("docker", "run", "-d", "--rm", "--name", s3, "-e", "AWS_ACCESS_KEY_ID=qualyx-test", "-e", "AWS_SECRET_ACCESS_KEY=qualyx-local-s3-secret", "-e", "S3_BUCKET=evidence", "-p", "127.0.0.1::8333", "chrislusf/seaweedfs:4.17", "mini", "-dir=/data")

            def port(name, number):
                return command("docker", "port", name, number).splitlines()[0].rsplit(":", 1)[1]

            env.update(QUALYX_DATABASE_URL=f"postgres://postgres:qualyx-local-test@127.0.0.1:{port(pg, '5432/tcp')}/postgres", QUALYX_API_TOKEN=TOKEN,
                       QUALYX_S3_BUCKET="evidence", QUALYX_S3_ENDPOINT=f"http://127.0.0.1:{port(s3, '8333/tcp')}",
                       QUALYX_S3_ACCESS_KEY="qualyx-test", QUALYX_S3_SECRET_KEY="qualyx-local-s3-secret", QUALYX_S3_ALLOW_HTTP="1")
            deadline = time.monotonic() + 60
            while subprocess.run(["docker", "exec", pg, "pg_isready", "-U", "postgres"], capture_output=True).returncode:
                assert time.monotonic() < deadline, "PostgreSQL startup timeout"
                time.sleep(0.25)
            while True:
                try:
                    urllib.request.urlopen(env["QUALYX_S3_ENDPOINT"], timeout=2)
                    break
                except urllib.error.HTTPError:
                    break  # Authentication-required S3 is reachable.
                except (OSError, TimeoutError):
                    assert time.monotonic() < deadline, "S3 startup timeout"
                    time.sleep(0.25)

            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                bind = f"127.0.0.1:{sock.getsockname()[1]}"
            base = "http://" + bind

            def start(*args):
                log = open(root / f"process-{len(logs)}.log", "w+", encoding="utf-8", errors="replace")
                logs.append(log)
                process = subprocess.Popen([str(BIN), *args], env=env, stdout=log, stderr=log, start_new_session=os.name != "nt")
                processes.append(process)
                return process

            def api():
                process = start("serve", "--team", "--root", str(root), "--bind", bind)
                deadline = time.monotonic() + 20
                while True:
                    assert process.poll() is None, "API exited; see process logs"
                    try:
                        request(base, "/health")
                        return process
                    except OSError:
                        assert time.monotonic() < deadline
                        time.sleep(0.1)

            passing, failing, slow = (root / name for name in ["passing", "failing", "slow"])
            shutil.copytree(REPO / "fixtures/python/passing", passing)
            shutil.copytree(REPO / "fixtures/python/failing", failing)
            shutil.copytree(REPO / "fixtures/python/passing", slow)
            (passing / "tests/test_worker_boundary.py").write_text("import os\ndef test_service_credentials_are_not_inherited():\n    assert not any(k.startswith('QUALYX_') for k in os.environ)\n")
            (passing / ".qualyx").mkdir()
            (passing / ".qualyx/config.toml").write_text('[run]\nenv_passthrough = ["QUALYX_DATABASE_URL", "QUALYX_API_TOKEN", "QUALYX_S3_SECRET_KEY"]\n')
            (slow / "tests/test_slow.py").write_text("import time\nfrom pathlib import Path\ndef test_slow():\n    Path('slow-started').write_text('started')\n    time.sleep(120)\n")
            def await_slow_test():
                deadline = time.monotonic() + 20
                while not (slow / "slow-started").exists():
                    assert time.monotonic() < deadline, "slow test was not collected/executed"
                    time.sleep(0.1)
            server = api()
            client = QualyxClient(base, TOKEN)
            job = client.start(passing)["run_id"]
            assert request(base, "/v1/runs/" + job)["state"] == "queued"
            assert not (passing / ".qualyx/runs").exists(), "API must not prepare or execute jobs"
            stop(server)
            server = api()
            assert request(base, "/v1/runs/" + job)["state"] == "queued", "queue survives API restart"
            workers = [start("worker", "--root", str(root), "--allow-process", "--once") for _ in range(2)]
            finished = await_state(base, job, {"finished", "failed"})
            assert finished["state"] == "finished" and finished["manifest"]["status"] == "passed", finished
            assert any(t["name"] == "test_service_credentials_are_not_inherited" for r in finished["manifest"]["runners"] for t in r["tests"]), "credential test must be collected, not silently ignored"
            assert client.status(job)["run_id"] == job
            assert client.runs(passing)["jobs"][0]["run_id"] == job
            for worker in workers:
                assert worker.wait(timeout=30) == 0
            assert len(list((passing / ".qualyx/runs").iterdir())) == 1, "atomic claim prevents duplicate execution"

            # Remove local evidence from its original location (preserved in this tempdir).
            evidence = passing / ".qualyx/runs" / job
            (evidence / "pytest.junit.xml").write_text("tampered evidence")
            refused = request(base, f"/v1/runs/{job}/artifacts/pytest.junit.xml?" + urlencode({"project": str(passing)}), expected=404)
            assert "hash mismatch" in refused["error"]
            evidence.rename(root / "saved-evidence")
            xml = client.artifact(job, passing, "pytest.junit.xml")
            assert "test_multiplies_two_numbers" in xml, "manifest + artifact restored from signed S3"
            bad = request(base, "/v1/runs", {"project": str(failing)}, 202)["run_id"]
            worker = start("worker", "--root", str(root), "--allow-process", "--once")
            result = await_state(base, bad, {"finished", "failed"})
            assert result["state"] == "finished" and result["manifest"]["status"] == "failed", result
            assert worker.wait(timeout=30) == 0

            cancelled = request(base, "/v1/runs", {"project": str(slow)}, 202)["run_id"]
            assert client.cancel(cancelled)["cancel_requested"]
            assert request(base, "/v1/runs/" + cancelled)["state"] == "cancelled"
            active = request(base, "/v1/runs", {"project": str(slow)}, 202)["run_id"]
            worker = start("worker", "--root", str(root), "--allow-process", "--once")
            await_state(base, active, {"running"})
            await_slow_test()
            request(base, f"/v1/runs/{active}/cancel", {}, 202)
            result = await_state(base, active, {"finished", "failed"})
            assert result["manifest"]["status"] != "passed", result
            assert worker.wait(timeout=30) == 0

            (slow / "slow-started").unlink()
            interrupted = request(base, "/v1/runs", {"project": str(slow)}, 202)["run_id"]
            worker = start("worker", "--root", str(root), "--allow-process", "--once")
            await_state(base, interrupted, {"running"})
            await_slow_test()
            stop(worker)
            result = await_state(base, interrupted, {"failed"}, timeout=25)
            assert "lease expired" in result["error"], result
            assert start("worker", "--root", str(root), "--allow-process", "--once").wait(timeout=20) == 0
            assert request(base, "/v1/runs/" + interrupted)["state"] == "failed", "no automatic rerun"
            queued = [client.start(passing)["run_id"] for _ in range(64)]
            request(base, "/v1/runs", {"project": str(passing)}, expected=503)
            for pending in queued:
                client.cancel(pending)
            stop(server)
            server = api()
            assert request(base, "/v1/runs/" + job)["manifest"]["status"] == "passed"
            print("PASS: durable queue, API restart, atomic workers, signed S3 restore, seeded failure, queued/running cancellation, lost-worker recovery")
        except BaseException:
            for log in logs:
                log.flush()
                log.seek(0)
                print(log.read()[-5000:])
            print(command("docker", "logs", s3)[-3000:] if subprocess.run(["docker", "inspect", s3], capture_output=True).returncode == 0 else "S3 container absent")
            raise
        finally:
            for process in reversed(processes):
                stop(process)
            for log in logs:
                log.close()
            for name in [s3, pg]:
                subprocess.run(["docker", "rm", "-f", name], capture_output=True)


if __name__ == "__main__":
    main()
