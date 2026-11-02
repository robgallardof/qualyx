//! Container-only QuickJS worker. Never invoke this binary with untrusted code on the host.
use rquickjs::{Context, Function, Promise, Runtime};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

const LIMIT: usize = 65_536;
fn read() -> Result<Value, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let n = io::stdin()
        .lock()
        .take((LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "read failed")?;
    if n == 0 || n > LIMIT || bytes.last() != Some(&b'\n') {
        return Err("frame limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid frame".into())
}
fn send(value: Value) -> Result<(), String> {
    let text = serde_json::to_string(&value).map_err(|_| "invalid JSON")?;
    if text.len() >= LIMIT {
        return Err("output limit".into());
    }
    let mut out = io::stdout().lock();
    writeln!(out, "{text}")
        .and_then(|_| out.flush())
        .map_err(|_| "write failed".into())
}
fn run() -> Result<(), String> {
    let request = read()?;
    let code = request["code"].as_str().ok_or("missing code")?;
    if code.len() > 16_384 {
        return Err("code limit".into());
    }
    let rt = Runtime::new().map_err(|_| "runtime failed")?;
    rt.set_memory_limit(16 * 1024 * 1024);
    rt.set_max_stack_size(256 * 1024);
    let started = Instant::now();
    rt.set_interrupt_handler(Some(Box::new(move || {
        started.elapsed() > Duration::from_secs(10)
    })));
    let context = Context::full(&rt).map_err(|_| "context failed")?;
    send(json!({"kind":"ready"}))?;
    let result = context.with(|ctx| -> rquickjs::Result<String> {
        ctx.globals().set("__bridge", Function::new(ctx.clone(), |request: String| -> String {
            let result = (|| {
                let mut value: Value = serde_json::from_str(&request).map_err(|_| "invalid call".to_string())?;
                value["kind"] = json!("call");
                send(value)?;
                read()
            })();
            serde_json::to_string(&result.unwrap_or_else(|_| json!({"ok":false,"error":"bridge failure"}))).unwrap()
        })?)?;
        // Capture the bridge in a closure, then remove its global name. Neither eval nor
        // constructor tricks gain new authority: there are no OS bindings or module loaders.
        ctx.eval::<(),_>(r#"globalThis.qualyx = Object.freeze((bridge => ({call: async (capability, input) => {
            const reply = JSON.parse(bridge(JSON.stringify({capability, input})));
            if (!reply.ok) throw new Error(reply.error);
            return reply.result;
        }}))(__bridge)); delete globalThis.__bridge;"#)?;
        let source = format!("(async () => {{ {code}\n }})().then(value => JSON.stringify(value === undefined ? null : value))");
        let promise: Promise = ctx.eval(source)?;
        promise.finish()
    }).map_err(|_| if started.elapsed() > Duration::from_secs(10) {"runtime timeout"} else {"JavaScript execution failed"})?;
    let value: Value = serde_json::from_str(&result).map_err(|_| "result is not JSON")?;
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let peak_rss_kib = status
        .lines()
        .find(|line| line.starts_with("VmHWM:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok());
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    // Fields after ')' start at field 3; utime/stime are fields 14/15. Report kernel
    // ticks without inventing a portable tick frequency or attributing Docker daemon CPU.
    let ticks: Vec<_> = stat
        .rsplit_once(')')
        .map(|(_, s)| s.split_whitespace().collect::<Vec<_>>())
        .unwrap_or_default();
    let cpu_ticks = ticks
        .get(11)
        .and_then(|s| s.parse::<u64>().ok())
        .zip(ticks.get(12).and_then(|s| s.parse::<u64>().ok()))
        .map(|(a, b)| a + b);
    send(
        json!({"kind":"result","result":value,"metrics":{"peak_rss_kib":peak_rss_kib,"cpu_ticks":cpu_ticks,"js_memory_bytes":rt.memory_usage().memory_used_size}}),
    )
}
fn main() {
    if let Err(reason) = run() {
        let _ = send(json!({"kind":"error","reason":reason}));
        std::process::exit(1);
    }
}
