use std::sync::Arc;
use zork_config::services::CueAccountConfig;
use zork_gui::desktop::account::{AccountCancellation, AccountFlow};
fn config(base: &str, mode: &str) -> CueAccountConfig {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    CueAccountConfig {
        issuer: format!("{base}/{mode}"),
        client_id: "zork-local-fixture".into(),
        redirect_uri: format!("http://127.0.0.1:{port}/oauth/callback"),
    }
}
#[test]
#[ignore = "run via scripts/test-cue-account.py local signed fixture"]
fn local_oidc_flow() {
    let base = std::env::var("ZORK_TEST_OIDC").unwrap();
    for mode in [
        "success",
        "badstate",
        "nonce",
        "issuer",
        "audience",
        "expired",
        "signature",
    ] {
        let flow = AccountFlow::prepare(
            config(&base, mode),
            Arc::new(AccountCancellation::new().unwrap()),
        )
        .unwrap();
        let url = flow.url.clone();
        let finish = std::thread::spawn(move || flow.finish());
        let response = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap()
            .get(url)
            .send()
            .unwrap();
        assert!(response.status().is_success());
        let result = finish.join().unwrap();
        if matches!(mode, "success" | "badstate") {
            let identity = result.unwrap();
            assert_eq!(identity.subject, "local-user");
            assert_eq!(identity.name, "Local Cue User");
            assert_eq!(identity.email.as_deref(), Some("fixture@example.test"));
        } else {
            assert!(result.is_err(), "accepted invalid {mode}");
        }
    }
    let cancel = Arc::new(AccountCancellation::new().unwrap());
    cancel.cancel();
    assert!(AccountFlow::prepare(config(&base, "success"), cancel).is_err());
    let cancel = Arc::new(AccountCancellation::new().unwrap());
    let flow = AccountFlow::prepare(config(&base, "success"), cancel.clone()).unwrap();
    cancel.cancel();
    assert!(flow.finish().is_err());
}
