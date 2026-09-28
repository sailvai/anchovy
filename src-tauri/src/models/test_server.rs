//! A tiny local HTTP file server for download tests. Supports `Range:
//! bytes=N-`, one redirect route, and cutting a response short to simulate a
//! dropped connection.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub path: String,
    pub range: Option<String>,
}

#[derive(Default)]
struct Shared {
    files: HashMap<String, Vec<u8>>,
    requests: Vec<Request>,
    /// When set, the next response sends full headers but only this many
    /// body bytes, then closes the connection.
    cut_next_after: Option<usize>,
    ignore_range: bool,
}

pub struct TestServer {
    port: u16,
    shared: Arc<Mutex<Shared>>,
}

impl TestServer {
    pub fn start() -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = shared.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = state.clone();
                thread::spawn(move || handle(stream, &state));
            }
        });
        TestServer { port, shared }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/{path}", self.port)
    }

    pub fn put(&self, path: &str, data: &[u8]) {
        let mut shared = self.shared.lock().unwrap();
        shared.files.insert(format!("/{path}"), data.to_vec());
    }

    pub fn cut_next_response_after(&self, bytes: usize) {
        self.shared.lock().unwrap().cut_next_after = Some(bytes);
    }

    pub fn ignore_range(&self) {
        self.shared.lock().unwrap().ignore_range = true;
    }

    pub fn requests(&self) -> Vec<Request> {
        self.shared.lock().unwrap().requests.clone()
    }
}

fn handle(mut stream: TcpStream, shared: &Mutex<Shared>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range = Some(value.trim().to_string());
            }
        }
    }

    let (file, cut, ignore_range) = {
        let mut shared = shared.lock().unwrap();
        shared.requests.push(Request {
            path: path.clone(),
            range: range.clone(),
        });
        let file = shared.files.get(&path).cloned();
        (file, shared.cut_next_after.take(), shared.ignore_range)
    };

    if let Some(target) = path.strip_prefix("/redirect") {
        let _ = write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        return;
    }
    let Some(data) = file else {
        let _ = write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        return;
    };

    let start = range
        .filter(|_| !ignore_range)
        .and_then(|range| {
            range
                .strip_prefix("bytes=")?
                .strip_suffix('-')?
                .parse()
                .ok()
        })
        .filter(|start: &usize| *start < data.len());
    let (status, body, content_range) = match start {
        Some(start) => (
            "206 Partial Content",
            &data[start..],
            format!(
                "Content-Range: bytes {start}-{}/{}\r\n",
                data.len() - 1,
                data.len()
            ),
        ),
        None => ("200 OK", &data[..], String::new()),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{content_range}Accept-Ranges: bytes\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let body = match cut {
        Some(bytes) => &body[..bytes.min(body.len())],
        None => body,
    };
    let _ = stream.write_all(body);
    let _ = stream.flush();
}
