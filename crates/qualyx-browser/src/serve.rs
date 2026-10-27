//! Loopback-only static file server for pages under test. Never binds a public interface and
//! never serves files outside its root.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::{Result, err};

enum Content {
    Dir(PathBuf),
    Html(String),
}

pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// Map a request path to a file under `root`, refusing traversal.
pub fn resolve(root: &Path, request_path: &str) -> Option<PathBuf> {
    let path = request_path.split(['?', '#']).next().unwrap_or("/");
    let rel = Path::new(path.trim_start_matches('/'));
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let mut full = root.join(rel);
    if full.is_dir() {
        full = full.join("index.html");
    }
    full.is_file().then_some(full)
}

fn respond(mut s: TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(body);
}

fn handle(s: TcpStream, content: &Content) {
    let mut line = String::new();
    if BufReader::new(&s).read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    if method != "GET" && method != "HEAD" {
        return respond(
            s,
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed",
        );
    }
    match content {
        Content::Html(h) => respond(s, "200 OK", "text/html; charset=utf-8", h.as_bytes()),
        Content::Dir(root) => {
            match resolve(root, path).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((p, body)) => respond(s, "200 OK", mime(&p), &body),
                None => respond(s, "404 Not Found", "text/plain", b"not found"),
            }
        }
    }
}

impl Server {
    fn start(content: Content) -> Result<Server> {
        let l = TcpListener::bind("127.0.0.1:0").map_err(err)?;
        let port = l.local_addr().map_err(err)?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let content = Arc::new(content);
        thread::spawn(move || {
            for s in l.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(s) = s {
                    let c = content.clone();
                    thread::spawn(move || handle(s, &c));
                }
            }
        });
        Ok(Server { port, stop })
    }

    pub fn start_dir(root: &Path) -> Result<Server> {
        let root = root
            .canonicalize()
            .map_err(|e| err(format!("{}: {e}", root.display())))?;
        Server::start(Content::Dir(root))
    }

    pub fn start_html(html: &str) -> Result<Server> {
        Server::start(Content::Html(html.to_string()))
    }

    pub fn url(&self, path: &str) -> String {
        format!(
            "http://127.0.0.1:{}/{}",
            self.port,
            path.trim_start_matches('/')
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port)); // unblock accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_traversal_and_serves_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("index.html"), "hi").unwrap();
        let root = d.path().canonicalize().unwrap();
        assert!(resolve(&root, "/").is_some());
        assert!(resolve(&root, "/index.html?x=1").is_some());
        for bad in [
            "/../secret",
            "/a/../../b",
            "/C:/Windows/win.ini",
            "//etc/passwd",
        ] {
            assert!(resolve(&root, bad).is_none(), "{bad}");
        }
        let s = Server::start_dir(d.path()).unwrap();
        let body = ureq::get(&s.url("/"))
            .call()
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();
        assert_eq!(body, "hi");
        let r = ureq::get(&s.url("/../x"))
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .unwrap();
        assert_eq!(r.status().as_u16(), 404);
    }
}
