//! Drives every browser installed on the machine through the login fixture.
//! Set QUALYX_TEST_BROWSERS=1 to require it; the test then fails when no browser is found.
//! QUALYX_TEST_BROWSERS_ONLY=chrome,firefox limits which ones run.

use std::path::{Path, PathBuf};

use qualyx_browser::{flow, serve::Server};

fn web(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/web")
        .join(rel)
}

fn enabled() -> bool {
    let on = std::env::var("QUALYX_TEST_BROWSERS").as_deref() == Ok("1");
    if !on {
        eprintln!("NOT RUN: set QUALYX_TEST_BROWSERS=1 to drive installed browsers");
    }
    on
}

fn browsers() -> Vec<qualyx_browser::Browser> {
    let only = std::env::var("QUALYX_TEST_BROWSERS_ONLY").ok();
    let list: Vec<_> = qualyx_browser::detect()
        .into_iter()
        .filter(|b| {
            only.as_deref()
                .is_none_or(|o| o.split(',').any(|x| x == b.id))
        })
        .collect();
    assert!(!list.is_empty(), "no browser detected on this machine");
    list
}

fn load(name: &str) -> flow::Flow {
    serde_json::from_slice(&std::fs::read(web(&format!("flows/{name}.json"))).unwrap()).unwrap()
}

#[test]
fn smoke_every_detected_browser() {
    if !enabled() {
        return;
    }
    for b in browsers() {
        let r = qualyx_browser::smoke(&b);
        eprintln!("{} {:?} {} {}", r.browser, r.version, r.status, r.detail);
        assert_eq!(r.status, "verified", "{r:?}");
    }
}

#[test]
fn login_flows_pass_and_fail_with_evidence() {
    if !enabled() {
        return;
    }
    let server = Server::start_dir(&web("site")).unwrap();
    let base = server.url("/");
    for b in browsers() {
        let out = tempfile::tempdir().unwrap();

        let mut d = qualyx_browser::launch(&b, true).unwrap();
        let ok = flow::run(&load("login-ok"), d.as_mut(), &b, out.path(), Some(&base));
        assert!(ok.passed, "{}: {:#?}", b.id, ok.steps);
        assert!(
            ok.screenshots.iter().any(|s| s.contains("signed-in")),
            "{ok:?}"
        );
        for s in &ok.screenshots {
            let png = std::fs::read(out.path().join(s)).unwrap();
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{s}");
        }
        if ok.protocol != "webdriver" {
            assert!(
                ok.console
                    .iter()
                    .any(|c| c.text.contains("login attempt") && c.text.contains("ok")),
                "{}: {:?}",
                b.id,
                ok.console
            );
        }
        if ok.protocol == "cdp" {
            assert!(
                ok.network_failures
                    .iter()
                    .any(|f| f.contains("missing-endpoint")),
                "{}: {:?}",
                b.id,
                ok.network_failures
            );
        }
        drop(d);

        let mut d = qualyx_browser::launch(&b, true).unwrap();
        let bad = flow::run(
            &load("login-wrong-password"),
            d.as_mut(),
            &b,
            out.path(),
            Some(&base),
        );
        assert!(!bad.passed);
        let failed = bad.steps.iter().find(|s| !s.passed).unwrap();
        assert_eq!(failed.index, 4, "{:#?}", bad.steps);
        assert!(
            failed
                .error
                .as_deref()
                .unwrap()
                .contains("Invalid credentials"),
            "{failed:?}"
        );
        assert!(failed.screenshot.is_some());
    }
}
