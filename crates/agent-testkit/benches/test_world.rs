use zork_agent_testkit::measure_virtual_sessions;

fn main() {
    let session_count = std::env::args()
        .nth(1)
        .map(|value| {
            value
                .parse::<usize>()
                .expect("session count must be an integer")
        })
        .unwrap_or(10_000);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("benchmark runtime");
    runtime.block_on(run(session_count));
}

async fn run(session_count: usize) {
    let result = measure_virtual_sessions(session_count).await;
    println!(
        "TestWorld: {} complete sessions, {} durable events, {:.3}s, {:.0} sessions/s",
        result.session_count,
        result.event_count,
        result.elapsed.as_secs_f64(),
        result.sessions_per_second()
    );
}
