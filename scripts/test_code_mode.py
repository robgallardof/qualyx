"""Real stdio MCP + Docker acceptance and repeatable runtime/context benchmark.

No LLM/provider is used. Run after building the CLI and sandbox image.
"""
import argparse
import json
import math
import os
from pathlib import Path
import queue
import shutil
import statistics
import subprocess
import tempfile
import threading
import time
import socket
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / ("qualyx.exe" if os.name == "nt" else "qualyx")


class Client:
    def __init__(self, root, enabled=True, approvals="", code_only=False):
        env = dict(os.environ, QUALYX_CODE_MODE="1" if enabled else "0",
                   QUALYX_CODE_APPROVALS=approvals,
                   QUALYX_CODE_ONLY="1" if code_only else "0",
                   QUALYX_TEST_SECRET="code-mode-secret-must-never-leak")
        self.process = subprocess.Popen([str(BIN), "mcp", "--root", str(root)],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, text=True, encoding="utf-8", env=env)
        self.messages = queue.Queue()
        def pump():
            for line in self.process.stdout:
                self.messages.put(json.loads(line))
            self.messages.put(None)
        threading.Thread(target=pump, daemon=True).start()
        self.seq = 0
        self.transcript = []
        self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                  "clientInfo": {"name": "qualyx-code-acceptance", "version": "1"}})
        self.send({"method": "notifications/initialized"})

    def send(self, value):
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", **value}) + "\n")
        self.process.stdin.flush()

    def receive(self, expected):
        deadline = time.monotonic() + 30
        while True:
            msg = self.messages.get(timeout=max(0.1, deadline-time.monotonic()))
            assert msg is not None, "MCP server exited"
            if msg.get("id") == expected:
                return msg

    def request(self, method, params):
        self.seq += 1
        self.send({"id": self.seq, "method": method, "params": params})
        msg = self.receive(self.seq)
        self.transcript.append({"request": {"method": method, "params": params}, "response": msg})
        assert "error" not in msg, msg
        return msg["result"]

    def tool(self, name, args, error=False, raw=False):
        result = self.request("tools/call", {"name": name, "arguments": args})
        assert bool(result.get("isError")) == error, result
        text = "\n".join(c.get("text", "") for c in result.get("content", []))
        if error or raw:
            return text
        return json.loads(text)

    def execute(self, code, error=False):
        return self.tool("qa_code_execute", {"code": code}, error=error)

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()


def acceptance(root):
    c = Client(root, enabled=False)
    try:
        assert not any(t["name"].startswith("qa_code_") for t in c.request("tools/list", {})["tools"])
        assert c.tool("qa_discover", {"path": str(root)})["root"]
    finally:
        c.close()
    c = Client(root)
    try:
        names = {t["name"] for t in c.request("tools/list", {})["tools"]}
        assert {"qa_code_search", "qa_code_execute", "qa_discover", "qa_run"} <= names
        found = c.tool("qa_code_search", {"query": "tests.run"})
        assert found["capabilities"][0]["id"] == "tests.run"
        assert c.execute("return {answer:6*7};")["result"] == {"answer": 42}
        assert c.execute("return ['process','require','fetch','WebSocket','Deno','std','os','__bridge'].map(k=>typeof globalThis[k]);")["result"] == ["undefined"]*8
        for payload in [
            "return process.env.QUALYX_TEST_SECRET;",
            "return require('fs').readFileSync('/etc/shadow');",
            "return require('child_process').execSync('id');",
            "return fetch('http://169.254.169.254/');",
            "return import('std');",
            "return await qualyx.call('shell', {command:'id'});",
            "return await qualyx.call('tests.run', {path:" + json.dumps(str(root)) + "});",
            "return await qualyx.call('tests.cancel', {run_id:'fake'});",
            "return await qualyx.call('tests.discover', {path:'/',approved:true});",
            "return await qualyx.call('tests.discover', {path:'/'});",
            "const a=[]; while(true) a.push(new Array(100000).fill('x'));",
            "return 'x'.repeat(100000);",
            "this is not valid JavaScript {{{",
            "while(true){}",
            "for(let i=0;i<33;i++) {try {await qualyx.call('unknown',{});}catch(e){}} return 1;",
        ]:
            c.execute(payload, error=True)
        # Two in-flight requests occupy both permits; a third must fail immediately.
        ids = []
        for _ in range(2):
            c.seq += 1
            ids.append(c.seq)
            c.send({"id": c.seq,"method":"tools/call","params":{"name":"qa_code_execute","arguments":{"code":"while(true){}"}}})
        time.sleep(1)
        assert "concurrency" in c.execute("return 1;", error=True)
        active_ids = subprocess.check_output(["docker","ps","--filter","name=qualyx-code-","--format","{{.ID}}"],text=True).split()
        assert len(active_ids)==2,active_ids
        for container_id in active_ids:
            inspected = json.loads(subprocess.check_output(["docker","inspect",container_id],text=True))[0]
            settings = inspected["HostConfig"]
            assert settings["NetworkMode"]=="none" and settings["ReadonlyRootfs"]
            assert settings["Memory"]==128*1024*1024 and settings["PidsLimit"]==16
            assert settings["CapDrop"]==["ALL"] and "no-new-privileges:true" in settings["SecurityOpt"]
            assert not inspected["Mounts"] and inspected["Config"]["User"]=="65532:65532"
            assert all("QUALYX_TEST_SECRET=" not in e for e in inspected["Config"]["Env"])
        for request_id in ids:
            c.send({"method":"notifications/cancelled","params":{"requestId":request_id,"reason":"acceptance cancellation"}})
        time.sleep(3)
        # Poll the engine, not just MCP's cancellation response.
        active = subprocess.check_output(["docker","ps","--filter","name=qualyx-code-","--format","{{.Names}}"], text=True)
        assert not active.strip(), active
        audit = (root / ".qualyx" / "code-audit.jsonl").read_text(encoding="utf-8")
        assert "code-mode-secret-must-never-leak" not in audit
        assert '"event":"cancelled"' in audit
        assert '"event":"timeout"' in audit
    finally:
        c.close()

    fixture = root / "seeded-failure"
    shutil.copytree(ROOT / "fixtures" / "python" / "failing", fixture,
                    ignore=shutil.ignore_patterns(".qualyx", "__pycache__", ".pytest_cache"))
    c = Client(root, approvals="tests.run,tests.cancel")
    try:
        path = json.dumps(str(fixture))
        submitted = c.execute(f"const p=await qualyx.call('tests.discover',{{path:{path}}});"
                              "if (!p.ecosystems.some(e=>e.ecosystem==='python')) throw Error('missing pytest');"
                              f"return await qualyx.call('tests.run',{{path:{path},timeout_secs:120}});")
        run_id = submitted["result"]["run_id"]
        deadline = time.monotonic()+150
        # Use existing direct polling to avoid wasting container startups while tests run.
        while True:
            status = c.tool("qa_run_status", {"run_id":run_id})
            if status["state"] != "running":
                break
            assert time.monotonic() < deadline, status
            time.sleep(1)
        assert status["status"] == "failed", status
        args = json.dumps({"path":str(fixture), "run_id":run_id})
        report = c.execute(f"const r=await qualyx.call('tests.results',{args});"
                           f"const a=await qualyx.call('artifacts.list',{args});"
                           f"const log=await qualyx.call('artifacts.read',{{...{args},name:'report.md'}});"
                           "return {status:r.status,failed:r.counts.failed,artifacts:a.artifacts,report:log.text};")["result"]
        assert report["status"] == "failed" and report["failed"] >= 1, report
        assert "test_multiplies_two_numbers" in report["report"], report
        assert report["artifacts"], report
        print("PASS: real MCP, sandbox denials/limits/cancellation, native pytest failing fixture, persisted evidence")
    finally:
        c.close()


def benchmark(root, output, repetitions):
    fixture = root / "benchmark-fixture"
    shutil.copytree(ROOT / "fixtures" / "python" / "failing", fixture,
                    ignore=shutil.ignore_patterns(".qualyx", "__pycache__", ".pytest_cache"))
    # Establish persisted real runner evidence once, outside both measured approaches.
    seeded = subprocess.run([str(BIN),"test",str(fixture),"--isolation","container","--timeout","120","--json"],
                            capture_output=True,text=True,encoding="utf-8",timeout=150)
    assert seeded.returncode==1,(seeded.stdout,seeded.stderr)
    manifest = json.loads(seeded.stdout)
    assert manifest["status"]=="failed" and manifest["counts"]["failed"]==1,manifest
    stored = {"path":str(fixture),"run_id":manifest["run_id"]}
    rows = []
    p = json.dumps(str(root))
    for scenario, count in [("simple",1),("dependent",2),("catalog_composition",3)]:
        for mode in ("direct", "code"):
            c = Client(root, enabled=mode == "code", code_only=mode == "code")
            try:
                tools = c.request("tools/list", {})["tools"]
                if mode == "code":
                    assert {t["name"] for t in tools} == {"qa_code_search","qa_code_execute"}
                schemas = tools if mode == "direct" else [t for t in tools if t["name"].startswith("qa_code_")]
                for _ in range(repetitions):
                    c.transcript = []
                    start = time.perf_counter()
                    result = None
                    if mode == "direct":
                        if scenario == "catalog_composition":
                            profile = c.tool("qa_discover", {"path":str(fixture)})
                            report = c.tool("qa_report", stored)
                            log = c.tool("qa_get_artifact", {**stored,"name":"report.md","max_bytes":8192},raw=True)
                            result = {"ecosystems":len(profile["ecosystems"]),"status":report["status"],"report":log}
                        else:
                            path = str(root)
                            for _ in range(count):
                                result = c.tool("qa_discover", {"path":path})
                                path = result["root"]
                    else:
                        if scenario == "catalog_composition":
                            c.tool("qa_code_search", {"query":"","limit":7})
                            result = c.execute("const p=await qualyx.call('tests.discover',"+json.dumps({"path":str(fixture)})+");"
                                               "const r=await qualyx.call('tests.results',"+json.dumps(stored)+");"
                                               "const log=await qualyx.call('artifacts.read',"+json.dumps({**stored,"name":"report.md"})+");"
                                               "return {ecosystems:p.ecosystems.length,status:r.status,report:log.text};")
                        else:
                            result = c.execute(f"let p={p}, result; for(let i=0;i<{count};i++) {{result=await qualyx.call('tests.discover',{{path:p}});p=result.root;}} return result;")
                    if scenario == "catalog_composition":
                        selected = result["result"] if mode=="code" else result
                        assert selected["status"]=="failed" and "test_multiplies_two_numbers" in selected["report"],selected
                    elapsed = (time.perf_counter()-start)*1000
                    dispatch_ms = None
                    if mode == "code":
                        events = [json.loads(line) for line in (root/".qualyx"/"code-audit.jsonl").read_text(encoding="utf-8").splitlines()]
                        dispatch_ms = sum(e["detail"]["duration_ms"] for e in events
                                          if e["execution_id"]==result["execution_id"] and e["event"]=="decision")
                    # UTF-8 bytes / 4 is only a reproducible context proxy, not tokenizer output.
                    schema_bytes = len(json.dumps(schemas,ensure_ascii=False).encode())
                    visible_bytes = schema_bytes + len(json.dumps(c.transcript,ensure_ascii=False).encode())
                    rows.append({"scenario":scenario,"mode":mode,"latency_ms":round(elapsed,3),
                                 "schema_bytes":schema_bytes,"visible_bytes":visible_bytes,
                                 "token_proxy_bytes_div_4":math.ceil(visible_bytes/4),
                                 "round_trips":len(c.transcript),"startup_ms":result.get("startup_ms"),
                                 "dispatch_ms":dispatch_ms,"workflows_per_second":round(1000/elapsed,3),
                                 "runtime_metrics":result.get("runtime_metrics"),"errors":0})
            finally:
                c.close()
    output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps({"environment":{"os":os.name,"docker":subprocess.check_output(["docker","version","--format","{{.Server.Version}}"],text=True).strip()},
                                  "limitations":["No model/provider latency or actual model token count.",
                                                  "CPU ticks and RSS cover sandbox process, not Docker daemon or test worker.",
                                                  "Small real catalog; no invented thousands of operations.",
                                                  "Sequential cold containers, warm image cache; throughput is reciprocal workflow latency."],"measurements":rows},indent=2)+"\n",encoding="utf-8")
    for scenario in ("simple","dependent","catalog_composition"):
        for mode in ("direct","code"):
            selected = [r for r in rows if r["scenario"]==scenario and r["mode"]==mode]
            print(scenario,mode,"median_ms",round(statistics.median(r["latency_ms"] for r in selected),2))


def team_acceptance(root):
    """Real HTTP -> Code Mode -> PostgreSQL -> separate native worker -> evidence."""
    fixture = root / "team-fixture"
    shutil.copytree(ROOT / "fixtures" / "python" / "failing", fixture,
                    ignore=shutil.ignore_patterns(".qualyx", "__pycache__", ".pytest_cache"))
    container = "qualyx-code-pg-" + uuid.uuid4().hex
    password = uuid.uuid4().hex
    api = worker = None
    subprocess.run(["docker", "run", "-d", "--rm", "--name", container,
                    "-e", "POSTGRES_PASSWORD="+password,"-e","POSTGRES_USER=qualyx",
                    "-e","POSTGRES_DB=qualyx","-p","127.0.0.1::5432","postgres:17-alpine"],check=True,capture_output=True)
    try:
        deadline = time.monotonic()+60
        while subprocess.run(["docker","exec",container,"pg_isready","-U","qualyx"],capture_output=True).returncode:
            assert time.monotonic() < deadline, "PostgreSQL not ready"
            time.sleep(0.5)
        address = subprocess.check_output(["docker","port",container,"5432"],text=True).strip()
        db_port = address.rsplit(":",1)[1]
        with socket.socket() as sock:
            sock.bind(("127.0.0.1",0))
            port = sock.getsockname()[1]
        token = uuid.uuid4().hex+uuid.uuid4().hex
        env = dict(os.environ,QUALYX_CODE_MODE="1",QUALYX_CODE_APPROVALS="tests.run,tests.cancel",
                   QUALYX_API_TOKEN=token,QUALYX_DATABASE_URL=f"postgres://qualyx:{password}@127.0.0.1:{db_port}/qualyx")
        # Optional archive variables from a developer's environment must not change this fixture.
        for key in list(env):
            if key.startswith("QUALYX_S3_"):
                del env[key]
        api = subprocess.Popen([str(BIN),"serve","--team","--bind",f"127.0.0.1:{port}","--root",str(root)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        base = f"http://127.0.0.1:{port}"
        deadline = time.monotonic()+30
        while True:
            try:
                urllib.request.urlopen(base+"/health",timeout=1).close()
                break
            except (OSError,urllib.error.URLError):
                assert time.monotonic()<deadline and api.poll() is None,"API did not start"
                time.sleep(0.2)
        def request(path, body=None, auth=True, origin=False):
            headers = {"Content-Type":"application/json"}
            if auth:
                headers["Authorization"]="Bearer "+token
            if origin:
                headers["Origin"]="http://localhost"
            req = urllib.request.Request(base+path,data=None if body is None else json.dumps(body).encode(),headers=headers)
            with urllib.request.urlopen(req,timeout=25) as response:
                return json.load(response)
        for kwargs, expected in [({"auth":False},401),({"origin":True},403)]:
            try:
                request("/v1/code/search",{},**kwargs)
                raise AssertionError("unauthorized HTTP accepted")
            except urllib.error.HTTPError as error:
                assert error.code==expected,error
        assert request("/v1/code/search",{"query":"tests.run"})["total"]==1
        code = "return await qualyx.call('tests.run',"+json.dumps({"path":str(fixture),"timeout_secs":120})+");"
        run = request("/v1/code/execute",{"code":code})["result"]
        assert run["state"]=="queued",run
        assert request("/v1/runs/"+run["run_id"])["state"]=="queued"
        # The seeded fixture is trusted. Explicitly use existing worker process permission;
        # JavaScript still runs exclusively in its separate restricted container.
        worker = subprocess.Popen([str(BIN),"worker","--root",str(root),"--allow-process","--once"],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        worker.wait(timeout=150)
        assert worker.returncode==0,"worker failed"
        status = request("/v1/runs/"+run["run_id"])
        assert status["state"]=="finished" and status["manifest"]["status"]=="failed",status
        args = json.dumps({"path":str(fixture),"run_id":run["run_id"]})
        result = request("/v1/code/execute",{"code":f"const r=await qualyx.call('tests.results',{args}); return {{failed:r.counts.failed,status:r.status}};"})["result"]
        assert result["failed"]==1 and result["status"]=="failed",result
        # Queue cancellation also travels through the authorized host bridge.
        queued = request("/v1/code/execute",{"code":code})["result"]
        cancel = "return await qualyx.call('tests.cancel',"+json.dumps({"run_id":queued["run_id"]})+");"
        assert request("/v1/code/execute",{"code":cancel})["result"]["cancel_requested"]
        assert request("/v1/runs/"+queued["run_id"])["cancel_requested"]
        print("PASS: authenticated HTTP, PostgreSQL queue, separate worker, seeded failure, result retrieval, queue cancellation")
    finally:
        for process in (worker,api):
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        subprocess.run(["docker","rm","-f",container],check=True,capture_output=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--benchmark",action="store_true")
    parser.add_argument("--team",action="store_true",help="Also verify real HTTP/PostgreSQL/separate-worker integration")
    parser.add_argument("--repetitions",type=int,default=3)
    parser.add_argument("--output",type=Path,default=ROOT/"artifacts"/"code-mode-benchmark.json")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="qualyx-code-") as directory:
        root = Path(directory)
        if args.benchmark:
            benchmark(root,args.output,args.repetitions)
        else:
            acceptance(root)
            if args.team:
                team_acceptance(root)
