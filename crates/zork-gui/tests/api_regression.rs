use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{stream, StreamExt};
use zork_gui::api::{ApiError, GatewayClient, MessagePage, Role, SseStream, TranscriptMessage};

#[test]
fn gateway_history_accepts_only_delivered_user_and_assistant_messages() {
    let page: MessagePage = serde_json::from_str(
        r#"{"items":[{"type":"message","role":"user","content":"hello"},{"type":"message","role":"assistant","content":"deliberate reply"}],"older_cursor":null}"#,
    )
    .expect("gateway-delivered messages must deserialize");

    assert!(matches!(
        page.items.as_slice(),
        [
            TranscriptMessage::Message {
                role: Role::User,
                content: user
            },
            TranscriptMessage::Message {
                role: Role::Assistant,
                content: assistant
            }
        ] if user == "hello" && assistant == "deliberate reply"
    ));

    for internal in [
        r#"{"type":"message","role":"mailbox","content":"internal mailbox"}"#,
        r#"{"type":"message","role":"tool","content":"internal tool result"}"#,
        r#"{"type":"wait","reason":"internal wait"}"#,
    ] {
        assert!(
            serde_json::from_str::<TranscriptMessage>(internal).is_err(),
            "the client-visible IM schema accepted an internal Agent event: {internal}"
        );
    }
}

#[test]
fn sse_parser_preserves_utf8_split_across_transport_chunks() {
    let frame = "event: message\ndata: {\"type\":\"message\",\"role\":\"assistant\",\"content\":\"你好\"}\n\n".as_bytes();
    let split = frame
        .windows("你".len())
        .position(|window| window == "你".as_bytes())
        .expect("Chinese text is present")
        + 1;
    let chunks = vec![
        Ok::<_, ApiError>(Bytes::copy_from_slice(&frame[..split])),
        Ok(Bytes::copy_from_slice(&frame[split..])),
    ];
    let mut events = SseStream::new(stream::iter(chunks));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");

    let event = runtime
        .block_on(events.next())
        .expect("one SSE event")
        .expect("valid SSE event");

    assert_eq!(event.name, "message");
    assert_eq!(
        event.data,
        r#"{"type":"message","role":"assistant","content":"你好"}"#
    );
}

#[test]
fn opening_sse_returns_and_delivers_data_while_connection_is_still_open() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept SSE request");
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).expect("read HTTP request");
            request.push(byte[0]);
        }
        assert!(
            String::from_utf8_lossy(&request).contains("/v1/im/sessions/live/events"),
            "GUI must subscribe to gateway-owned IM events"
        );
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\nevent: message\ndata: {\"type\":\"message\",\"role\":\"assistant\",\"content\":\"live\"}\n\n",
            )
            .expect("write SSE response");
        socket.flush().expect("flush SSE response");
        thread::sleep(Duration::from_millis(600));
    });

    let (result_tx, result_rx) = mpsc::channel();
    let client_thread = thread::spawn(move || {
        let client = GatewayClient::new(format!("http://{address}"), None);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let result = runtime.block_on(async {
            let mut events = client.stream_events("live").await?;
            let event = events.next().await.transpose()?.expect("one live event");
            Ok::<_, ApiError>((event.name, event.data))
        });
        result_tx.send(result).expect("send client result");
    });

    let early_result = result_rx.recv_timeout(Duration::from_millis(300));
    server.join().expect("test server thread");
    client_thread.join().expect("client thread");

    let (name, data) = early_result.expect(
        "stream_events must return and deliver the first event before the server closes the SSE connection",
    )
    .expect("SSE request succeeds");
    assert_eq!(name, "message");
    assert_eq!(
        data,
        r#"{"type":"message","role":"assistant","content":"live"}"#
    );
}
