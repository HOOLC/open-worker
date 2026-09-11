use zork_client_types::comments::{compose_document, CommentSource, DraftComment, TextAttachment};

pub fn content(i: usize) -> String {
    let basic=[
        "普通中英文消息，缓存必须可用。 Plain text 👋",
        "# Heading\n\n## Second\n\n### Third\n\nParagraph **bold** *italic* ~~strike~~ `code`.",
        "- item one\n- item two\n  - nested\n\n1. ordered\n2. second\n\n- [x] done\n- [ ] pending",
        "> quotation\n>\n> second line\n\n---\n\n[link](https://example.test/fixture) <https://example.test/path>",
        "```rust\nfn fixture() { println!(\"different code\"); }\n```\n\n    indented code",
        "| Name | Value |\n|:---|---:|\n| 中文 | 123 |\n| row | value |",
        "![preview](https://example.test/image.png)\n\n[文件.pdf](https://example.test/file.pdf)",
        "Inline $a^2+b^2=c^2$\n\n$$\\sum_i x_i$$\n\n```mermaid\ngraph LR\nA-->B\n```",
        "[reference][r]\n\n[r]: https://example.test/ref\n\nline  \nhard break\n\n<em>HTML fallback</em>",
    ];
    match i % 12 {
        9 => compose_document(
            "带批注",
            &[DraftComment {
                id: format!("c{i}"),
                source: CommentSource {
                    session_id: "chat".into(),
                    message_id: Some(format!("m{i}")),
                    quote: "原文".into(),
                    ..Default::default()
                },
                comment: "保留用户批注".into(),
            }],
            &[],
        ),
        10 => compose_document(
            "文本附件",
            &[],
            &[TextAttachment {
                id: format!("f{i}"),
                name: format!("file-{i}.txt"),
                content: "file content\n第二行".into(),
            }],
        ),
        11 => format!(
            "Long message {i}\n{}",
            "多段落 mixed content **bold** `code`\n\n".repeat(128)
        ),
        n => format!("record {i}\n{}", basic[n]),
    }
}
