//! Explicit embedded-browser smoke test. Uses a fresh profile and closes it on exit.
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
use zork_browser::{Action, Browser};
fn main() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let server = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", server.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in server.incoming().flatten() {
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let body="<!doctype html><title>Zork browser probe</title><input id='input' oninput=\"document.querySelector('#typed').textContent=this.value\"><p id='typed'></p><button id='button' onclick=\"document.querySelector('#result').textContent='clicked'\">Click me</button><p id='result'>ready</p><a href='/second'>Next page</a><p id='identity'></p><script>document.querySelector('#identity').textContent='webdriver='+navigator.webdriver+' UA='+navigator.userAgent+' cookie='+document.cookie;document.cookie='zork_probe=present;max-age=3600;path=/';document.querySelector('#identity').textContent+=' after='+document.cookie;</script>";
            let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);
        }
    });
    let profile = std::env::var_os("ZORK_BROWSER_PROBE_PROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.path().join("cef"));
    let browser = Browser::new(profile);
    let result = browser.execute(
        "probe",
        Action::Open {
            url: origin.clone(),
        },
    )?;
    let id = result["tab"]["id"].as_str().unwrap().to_owned();
    let started = Instant::now();
    loop {
        if browser.execute("probe", Action::Read { tab_id: id.clone() })?["page"]["title"]
            == "Zork browser probe"
        {
            break;
        }
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(15),
            "page did not load"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    browser.viewport("probe", &id, 600, 500, true)?;
    let frame_deadline = Instant::now() + Duration::from_secs(5);
    while browser.frame("probe", &id).is_none() {
        anyhow::ensure!(
            Instant::now() < frame_deadline,
            "browser did not deliver a display frame"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    browser.execute(
        "probe",
        Action::Click {
            tab_id: id.clone(),
            selector: "#button".into(),
        },
    )?;
    let read = browser.execute("probe", Action::Read { tab_id: id.clone() })?;
    anyhow::ensure!(
        read["page"]["text"].as_str().unwrap().contains("clicked"),
        "click failed"
    );
    browser.execute(
        "probe",
        Action::Type {
            tab_id: id.clone(),
            selector: "#input".into(),
            text: "中文 input".into(),
        },
    )?;
    let typed = browser.execute("probe", Action::Read { tab_id: id.clone() })?;
    let text = typed["page"]["text"].as_str().unwrap();
    anyhow::ensure!(text.contains("中文 input"), "input text not delivered");
    anyhow::ensure!(
        text.contains("webdriver=false") && !text.contains("HeadlessChrome"),
        "launch changed browser identity: {text}"
    );
    let selected = browser.inspect("probe", &id, 10., 10.)?;
    anyhow::ensure!(!selected.selector.is_empty(), "inspection failed");
    let screenshot = browser.execute("probe", Action::Screenshot { tab_id: id.clone() })?;
    anyhow::ensure!(
        screenshot["base64"].as_str().unwrap().len() > 100,
        "screenshot empty"
    );
    anyhow::ensure!(
        browser
            .execute("other", Action::Read { tab_id: id.clone() })
            .is_err(),
        "cross-conversation access allowed"
    );
    browser.execute(
        "probe",
        Action::Navigate {
            tab_id: id.clone(),
            url: format!("{origin}/second"),
        },
    )?;
    browser.execute("probe", Action::Back { tab_id: id.clone() })?;
    browser.execute("probe", Action::Close { tab_id: id })?;
    browser.shutdown();
    let reopened = browser.execute(
        "probe",
        Action::Open {
            url: origin.clone(),
        },
    )?;
    let reopened_id = reopened["tab"]["id"].as_str().unwrap().to_owned();
    let restored = browser.execute(
        "probe",
        Action::Read {
            tab_id: reopened_id,
        },
    )?;
    anyhow::ensure!(
        restored["page"]["text"]
            .as_str()
            .unwrap()
            .contains("cookie=zork_probe=present"),
        "browser session cookie did not survive restart: {}",
        restored["page"]["text"]
    );
    let owned_tabs = browser.tabs("probe").len();
    browser.open_downloads()?;
    anyhow::ensure!(
        browser.tabs("probe").len() == owned_tabs,
        "downloads leaked into conversation tabs"
    );
    if let Some(path) = std::env::var_os("ZORK_BROWSER_PROBE_REPORT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "status":"passed","embedded_cef":true,"webdriver":false,"default_user_agent":true,
                "persistent_cookie_restart":true,"native_paint":true,"navigation":true,"click":true,
                "text_input":true,"inspection":true,"screenshot":true,"conversation_isolation":true,
                "downloads_kept_outside_agent_tabs":true
            }))?,
        )?;
    }
    println!("PASS: persistent session, default CEF identity, private embedded-browser control, navigation, read, click, type, inspection, screenshot, conversation isolation, close");
    Ok(())
}
