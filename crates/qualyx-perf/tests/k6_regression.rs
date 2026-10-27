//! Real k6 against an in-process server whose latency is seeded. Opt in with QUALYX_TEST_K6=1
//! (k6 must be on PATH).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use qualyx_perf::{Limits, Workload, authorize, compare, load_latencies, run};

/// HTTP/1.1 server answering every request after `delay` ms.
fn server(delay: Arc<AtomicU64>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let delay = delay.clone();
            thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut s = s;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    loop {
                        let mut h = String::new();
                        if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                            break;
                        }
                    }
                    thread::sleep(Duration::from_millis(delay.load(Ordering::SeqCst)));
                    if s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\ncontent-type: text/plain\r\n\r\nok").is_err() {
                        return;
                    }
                }
            });
        }
    });
    port
}

fn k6() -> Option<std::path::PathBuf> {
    if std::env::var("QUALYX_TEST_K6").as_deref() != Ok("1") {
        eprintln!("NOT RUN: set QUALYX_TEST_K6=1 with k6 on PATH");
        return None;
    }
    Some(which::which("k6").expect("QUALYX_TEST_K6=1 but k6 is not on PATH"))
}

fn workload(port: u16) -> Workload {
    let w: Workload = toml::from_str(&format!(
        "name = \"seeded\"\ntarget = \"http://127.0.0.1:{port}/api\"\nvus = 4\nduration_secs = 4\nwarmup_secs = 1\n"
    ))
    .unwrap();
    authorize(&w, &Limits::default()).unwrap();
    w
}

#[test]
fn detects_a_seeded_latency_regression_and_not_a_null_change() {
    let Some(k6) = k6() else { return };
    let delay = Arc::new(AtomicU64::new(3));
    let w = workload(server(delay.clone()));
    let root = tempfile::tempdir().unwrap();

    let base_dir = root.path().join("base");
    let base = run(&w, &k6, &base_dir, "base", 2).unwrap();
    let null_dir = root.path().join("null");
    let null = run(&w, &k6, &null_dir, "null", 2).unwrap();
    delay.store(15, Ordering::SeqCst); // seeded regression: +12 ms per request
    let slow_dir = root.path().join("slow");
    let slow = run(&w, &k6, &slow_dir, "slow", 2).unwrap();

    for r in [&base, &null, &slow] {
        assert!(
            r.samples
                .iter()
                .all(|s| s.error_rate == 0.0 && s.requests > 50),
            "{r:#?}"
        );
        assert!(r.k6_version.is_some());
    }
    let (bl, nl, sl) = (
        load_latencies(&base_dir, &base),
        load_latencies(&null_dir, &null),
        load_latencies(&slow_dir, &slow),
    );

    let regression = compare(&base, &bl, &slow, &sl, 0.01, 0.10);
    eprintln!("seeded: {regression:#?}");
    assert_eq!(regression.verdict, "regression", "{regression:#?}");
    assert!(regression.median_ratio > 1.5);

    let same = compare(&base, &bl, &null, &nl, 0.01, 0.10);
    eprintln!("null: {same:#?}");
    assert_ne!(same.verdict, "regression", "{same:#?}");
}

#[test]
fn abort_condition_stops_a_failing_target_early() {
    let Some(k6) = k6() else { return };
    // Nothing listens on this port: every request fails, so the abort threshold fires.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut w = workload(port);
    w.duration_secs = 60;
    let root = tempfile::tempdir().unwrap();
    let t = std::time::Instant::now();
    let r = run(&w, &k6, root.path(), "abort", 1).unwrap();
    assert!(
        t.elapsed() < Duration::from_secs(40),
        "abort did not trigger: {:?}",
        t.elapsed()
    );
    assert!(r.samples[0].error_rate > 0.5);
    assert!(
        r.samples[0]
            .thresholds_failed
            .iter()
            .any(|f| f.contains("http_req_failed")),
        "{:?}",
        r.samples[0]
    );
}
