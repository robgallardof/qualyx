//! OWASP ZAP baseline scan against a local page with missing security headers.
//! Opt in with QUALYX_TEST_ZAP=1 (needs docker and the ZAP image).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use qualyx_security::dast;

/// Serves a small HTML page with no CSP / X-Content-Type-Options headers (seeded weakness).
fn weak_site() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            thread::spawn(move || {
                let mut line = String::new();
                let _ = BufReader::new(&s).read_line(&mut line);
                let body = "<!doctype html><html><head><title>weak</title></head><body><a href=\"/about\">about</a><form action=\"/login\" method=\"post\"><input name=\"u\"></form></body></html>";
                let mut s = s;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    port
}

#[test]
fn baseline_finds_missing_security_headers() {
    if std::env::var("QUALYX_TEST_ZAP").as_deref() != Ok("1") {
        eprintln!(
            "NOT RUN: set QUALYX_TEST_ZAP=1 (docker + {})",
            dast::ZAP_IMAGE
        );
        return;
    }
    let port = weak_site();
    let out = tempfile::tempdir().unwrap();
    let r = dast::baseline(
        &format!("http://127.0.0.1:{port}/"),
        &[],
        out.path(),
        Duration::from_secs(900),
    )
    .unwrap();
    for f in &r.findings {
        eprintln!("{} {} {}", f.rule_id, f.level, f.message);
    }
    assert_eq!(r.status, "findings");
    // 10038: CSP header not set; 10021: X-Content-Type-Options missing.
    assert!(
        r.findings
            .iter()
            .any(|f| f.rule_id == "10038" || f.rule_id == "10021"),
        "{:#?}",
        r.findings
    );
}

#[test]
fn unauthorized_target_never_reaches_docker() {
    let out = tempfile::tempdir().unwrap();
    let e = dast::baseline(
        "https://example.com/",
        &[],
        out.path(),
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(e.contains("not loopback"), "{e}");
    assert!(
        std::fs::read_dir(out.path()).unwrap().next().is_none(),
        "nothing may run"
    );
}
