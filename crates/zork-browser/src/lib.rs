//! A client-owned embedded CEF browser. Credentials stay on this device.
mod cdp;
pub mod protocol;
mod service;
pub use protocol::{Action, Command, Inspection};
pub use service::{Browser, Frame, Tab};

/// The human address bar accepts searches; Agent navigation remains URL-only.
pub fn address_url(input: &str) -> anyhow::Result<String> {
    let input = input.trim();
    if input.contains(':') || (input.contains('.') && !input.chars().any(char::is_whitespace)) {
        return normalize_url(input);
    }
    anyhow::ensure!(
        !input.is_empty() && input.len() <= 8192 && !input.chars().any(char::is_control),
        "请输入网址或搜索内容"
    );
    let mut url = reqwest::Url::parse("https://www.google.com/search")?;
    url.query_pairs_mut().append_pair("q", input);
    Ok(url.into())
}

pub fn normalize_url(input: &str) -> anyhow::Result<String> {
    use anyhow::ensure;
    let input = input.trim();
    ensure!(
        !input.is_empty() && input.len() <= 8192 && !input.chars().any(char::is_control),
        "请输入有效的网址"
    );
    let candidate = if input.contains("://") {
        input.to_owned()
    } else if input.starts_with("localhost:") || input.starts_with("127.0.0.1:") {
        format!("http://{input}")
    } else {
        format!("https://{input}")
    };
    let url = reqwest::Url::parse(&candidate)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "只支持不含凭据的 HTTP 或 HTTPS 网址"
    );
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn human_address_bar_distinguishes_searches_from_navigation() {
        let search = reqwest::Url::parse(&address_url("中文 & browser").unwrap()).unwrap();
        assert_eq!(search.host_str(), Some("www.google.com"));
        assert_eq!(search.query_pairs().next().unwrap().1, "中文 & browser");
        assert_eq!(
            address_url("example.com/guide").unwrap(),
            "https://example.com/guide"
        );
        assert_eq!(
            address_url("localhost:3000").unwrap(),
            "http://localhost:3000/"
        );
        assert!(address_url("javascript:alert(1)").is_err());
        assert!(address_url("file:///etc/passwd").is_err());
    }
    #[test]
    fn only_web_urls_without_embedded_credentials_enter_the_browser() {
        assert_eq!(
            normalize_url("example.com/path").unwrap(),
            "https://example.com/path"
        );
        assert_eq!(
            normalize_url("localhost:3000/").unwrap(),
            "http://localhost:3000/"
        );
        for url in [
            "",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,test",
            "https://user:password@example.com",
            "https://example.com/a\nb",
        ] {
            assert!(normalize_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn action_arguments_cannot_supply_another_conversation_or_raw_script() {
        assert!(serde_json::from_str::<Command>(
            r#"{"request_id":"a","action":{"op":"read","tab_id":"tab"}}"#
        )
        .is_ok());
        for raw in [
            r#"{"request_id":"a","action":{"op":"read","tab_id":"tab","host":"other"}}"#,
            r#"{"request_id":"a","action":{"op":"evaluate","script":"document.cookie"}}"#,
            r#"{"request_id":"a","action":{"op":"click"}}"#,
        ] {
            assert!(serde_json::from_str::<Command>(raw).is_err());
        }
    }
}
