use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

struct Captured {
    method: String,
    path: String,
    body: String,
}

fn start_mock(
    handler: impl Fn(&Captured) -> (u16, String) + Send + Sync + 'static,
) -> (String, Arc<Mutex<Vec<Captured>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let stored = requests.clone();
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let captured = match read_http(&mut stream) {
                Some(captured) => captured,
                None => continue,
            };
            let (status, body) = handler(&captured);
            stored.lock().unwrap().push(captured);
            let reason = if (200..300).contains(&status) {
                "OK"
            } else {
                "ERR"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://{addr}"), requests)
}

fn read_http(stream: &mut std::net::TcpStream) -> Option<Captured> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    let mut header_end = None;
    let mut content_length = 0usize;
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = find_headers_end(&buf) {
                        header_end = Some(pos);
                        content_length = content_length_of(&buf[..pos]);
                    }
                }
                if let Some(pos) = header_end {
                    if buf.len() >= pos + content_length {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    let header_end = header_end?;
    let headers = std::str::from_utf8(&buf[..header_end]).ok()?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let path = target
        .split_once('?')
        .map_or(target.as_str(), |(path, _)| path)
        .to_owned();
    let body = buf
        .get(header_end + 4..header_end + 4 + content_length)
        .or_else(|| buf.get(header_end + 4..))
        .map(|slice| String::from_utf8_lossy(slice).into_owned())
        .unwrap_or_default();
    Some(Captured { method, path, body })
}

fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length_of(headers: &[u8]) -> usize {
    let text = String::from_utf8_lossy(headers);
    for line in text.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

fn zork_gh() -> Command {
    Command::new(env!("CARGO_BIN_EXE_zork-gh"))
}

#[test]
fn gh_wrapper_sets_token_and_strips_inherited_github_token() {
    let dir = tempfile::tempdir().unwrap();
    let capture = dir.path().join("capture");
    std::fs::create_dir_all(&capture).unwrap();
    let fake_gh = dir.path().join("real-gh");
    std::fs::write(
        &fake_gh,
        "#!/bin/sh\nprintf '%s' \"$GH_TOKEN\" > \"$CAPTURE_DIR/gh_token\"\nif [ -n \"${GITHUB_TOKEN+x}\" ]; then printf '%s' \"$GITHUB_TOKEN\" > \"$CAPTURE_DIR/github_token\"; fi\nprintf '%s\\n' \"$@\" > \"$CAPTURE_DIR/argv\"\npwd > \"$CAPTURE_DIR/cwd\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let (url, _) = start_mock(|request| {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/github-token/resolve");
        assert!(serde_json::from_str::<Value>(&request.body).unwrap()["command"].is_array());
        (
            200,
            json!({ "ok": true, "token": "starter-token" }).to_string(),
        )
    });
    let output = zork_gh()
        .args(["pr", "create", "--fill"])
        .current_dir(dir.path())
        .env("BROKER_API_BASE", &url)
        .env("BROKER_REAL_GH_PATH", &fake_gh)
        .env("CAPTURE_DIR", &capture)
        .env("GH_TOKEN", "inherited-gh-token")
        .env("GITHUB_TOKEN", "inherited-github-token")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(capture.join("gh_token")).unwrap(),
        "starter-token"
    );
    assert!(!capture.join("github_token").exists());
    assert_eq!(
        std::fs::read_to_string(capture.join("argv"))
            .unwrap()
            .trim(),
        "pr\ncreate\n--fill"
    );
}

#[test]
fn gh_wrapper_does_not_exec_real_gh_when_broker_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let ran = dir.path().join("ran");
    let fake_gh = dir.path().join("real-gh");
    std::fs::write(&fake_gh, format!("#!/bin/sh\ntouch {}\n", ran.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let (url, _) = start_mock(|request| {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/github-token/resolve");
        assert!(serde_json::from_str::<Value>(&request.body).unwrap()["command"].is_array());
        (
            409,
            json!({
                "ok": false,
                "message": "GitHub token for alice is invalid."
            })
            .to_string(),
        )
    });
    let output = zork_gh()
        .args(["pr", "create"])
        .env("BROKER_API_BASE", &url)
        .env("BROKER_REAL_GH_PATH", &fake_gh)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("GitHub token for alice is invalid."));
    assert!(!ran.exists());
}
