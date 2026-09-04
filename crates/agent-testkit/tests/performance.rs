use std::time::Duration;

use zork_agent_testkit::{
    measure_history_fragment_queries, measure_query_apis, measure_virtual_sessions,
};

#[tokio::test(flavor = "multi_thread")]
// Contract: docs/zork-agent-architecture.md [PERF-01]
async fn virtual_world_sustains_a_thousand_complete_session_lifecycles() {
    const SESSION_COUNT: usize = 1_000;
    const EVENTS_PER_SESSION: usize = 9;
    const MAX_DEBUG_DURATION: Duration = Duration::from_secs(5);

    let result = measure_virtual_sessions(SESSION_COUNT).await;

    assert_eq!(result.session_count, SESSION_COUNT);
    assert_eq!(result.event_count, SESSION_COUNT * EVENTS_PER_SESSION);
    assert!(
        result.elapsed <= MAX_DEBUG_DURATION,
        "TestWorld completed {} sessions in {:.3}s ({:.0} sessions/s), exceeding the {:?} debug/CI regression limit",
        result.session_count,
        result.elapsed.as_secs_f64(),
        result.sessions_per_second(),
        MAX_DEBUG_DURATION
    );
}

#[test]
// Contract: docs/zork-agent-architecture.md [QUERY-01, PERF-04]
fn real_file_query_apis_have_a_debug_regression_baseline() {
    let measured = measure_query_apis(32, 128, 20).expect("measure SessionQuery APIs");
    let per_call_limit = Duration::from_millis(100);

    for (name, duration) in [
        ("last_commit", measured.last_commit),
        ("snapshot_window", measured.snapshot_window),
        ("history_after", measured.history_after),
        ("history_before", measured.history_before),
        ("event_lookup", measured.event_lookup),
        ("discovery", measured.discovery),
        ("exists", measured.exists),
    ] {
        assert!(
            duration <= per_call_limit,
            "{name} averaged {duration:?}, exceeding the {per_call_limit:?} debug regression limit"
        );
    }
    assert!(
        measured.all_commits_per_second() >= 5_000.0,
        "all_commits_forward processed only {:.0} events/s",
        measured.all_commits_per_second()
    );
}

#[test]
// Contract: docs/zork-agent-architecture.md [QUERY-01, PERF-04]
fn long_session_fragments_sustain_a_thousand_serial_and_parallel_queries() {
    const MIN_QUERIES_PER_SECOND: f64 = 20.0;
    let measured = measure_history_fragment_queries(2, 4_096, 256, 1_024, 8)
        .expect("measure long history fragments");
    assert_eq!(measured.event_count, 8_192);
    assert_eq!(measured.query_count, 1_024);
    assert_eq!(measured.fragment_len, 256);
    assert!(
        measured.serial_queries_per_second() >= MIN_QUERIES_PER_SECOND,
        "serial history fragments sustained {:.1} queries/s",
        measured.serial_queries_per_second(),
    );
    assert!(
        measured.parallel_queries_per_second() >= MIN_QUERIES_PER_SECOND,
        "parallel history fragments sustained {:.1} queries/s",
        measured.parallel_queries_per_second(),
    );
}
