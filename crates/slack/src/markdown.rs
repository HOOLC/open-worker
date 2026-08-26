pub fn markdownish_to_mrkdwn(text: &str) -> String {
    let (protected, segments) = protect_code(text);
    restore_code(&transform(&protected), &segments)
}

pub fn chunk_slack_message(text: &str, chunk_size: usize) -> Vec<String> {
    let normalized = text.trim();
    if normalized.len() <= chunk_size {
        return vec![normalized.to_string()];
    }
    let mut chunks = Vec::new();
    let mut rest = normalized;
    while !rest.is_empty() {
        if rest.len() <= chunk_size {
            chunks.push(rest.to_string());
            break;
        }
        let mut end = chunk_size.min(rest.len());
        while end > 0 && !rest.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            end = rest
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(rest.len());
        }
        let window = &rest[..end];
        let split = window.rfind('\n').filter(|rel| *rel > 0).unwrap_or(end);
        chunks.push(rest[..split].to_string());
        rest = rest[split..].trim_start_matches('\n');
    }
    chunks
}

fn protect_code(text: &str) -> (String, Vec<(String, String)>) {
    let mut segments = Vec::new();
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("```") {
            if let Some(end) = stripped.find("```") {
                let fence_len = 3 + end + 3;
                let token = format!("\u{0000}CODE{}\u{0001}", segments.len());
                segments.push((token.clone(), rest[..fence_len].to_string()));
                out.push_str(&token);
                rest = &rest[fence_len..];
                continue;
            }
        }
        if rest.starts_with('`') {
            if let Some(end) = rest[1..].find('`') {
                let code_len = 1 + end + 1;
                let token = format!("\u{0000}CODE{}\u{0001}", segments.len());
                segments.push((token.clone(), rest[..code_len].to_string()));
                out.push_str(&token);
                rest = &rest[code_len..];
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    (out, segments)
}

fn restore_code(text: &str, segments: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (token, replacement) in segments {
        out = out.replace(token, replacement);
    }
    out
}

fn transform(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let mut line = line.to_string();
        if let Some(stripped) = line.strip_prefix("###### ") {
            line = format!("*{stripped}*");
        } else if let Some(stripped) = line.strip_prefix("##### ") {
            line = format!("*{stripped}*");
        } else if let Some(stripped) = line.strip_prefix("#### ") {
            line = format!("*{stripped}*");
        } else if let Some(stripped) = line.strip_prefix("### ") {
            line = format!("*{stripped}*");
        } else if let Some(stripped) = line.strip_prefix("## ") {
            line = format!("*{stripped}*");
        } else if let Some(stripped) = line.strip_prefix("# ") {
            line = format!("*{stripped}*");
        }
        line = replace_links(&line);
        line = line.replace("***", "*");
        line = replace_wrapped(&line, "**", "*");
        line = line.replace("~~", "~");
        if let Some(stripped) = line.strip_prefix("- ") {
            line = format!("• {stripped}");
        } else if let Some(stripped) = line.strip_prefix("* ") {
            line = format!("• {stripped}");
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn replace_wrapped(text: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut remaining = text;
    while let Some(index) = remaining.find(from) {
        out.push_str(&remaining[..index]);
        out.push_str(to);
        remaining = &remaining[index + from.len()..];
    }
    out.push_str(remaining);
    out
}

fn replace_links(text: &str) -> String {
    let mut out = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find('[') {
        if let Some(mid) = remaining[start + 1..].find("](") {
            let label_end = start + 1 + mid;
            let rest = &remaining[label_end + 2..];
            if let Some(end) = rest.find(')') {
                let label = &remaining[start + 1..label_end];
                let url = &rest[..end];
                out.push_str(&remaining[..start]);
                out.push('<');
                out.push_str(url);
                out.push('|');
                out.push_str(label);
                out.push('>');
                remaining = &rest[end + 1..];
                continue;
            }
        }
        let next = start + '['.len_utf8();
        out.push_str(&remaining[..next]);
        remaining = &remaining[next..];
    }
    out.push_str(remaining);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_markdownish() {
        let out = markdownish_to_mrkdwn("**hello** [x](https://x.test)\n- item");
        assert!(out.contains("*hello*"));
        assert!(out.contains("<https://x.test|x>"));
        assert!(out.contains("• item"));
    }

    #[test]
    fn converts_chinese_without_panic() {
        assert_eq!(markdownish_to_mrkdwn("你好"), "你好");
        assert_eq!(markdownish_to_mrkdwn("**你好**"), "*你好*");
        assert_eq!(
            markdownish_to_mrkdwn("看 [文档](https://x.test) 你好"),
            "看 <https://x.test|文档> 你好"
        );
    }

    #[test]
    fn chunks_long_text() {
        let chunks = chunk_slack_message(&"a".repeat(20), 8);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 8));
    }

    #[test]
    fn chunks_chinese_on_char_boundaries() {
        let chunks = chunk_slack_message(&"你好世界".repeat(10), 8);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.is_char_boundary(chunk.len()));
            assert!(!chunk.is_empty());
        }
        assert_eq!(chunks.concat(), "你好世界".repeat(10));
    }
}
