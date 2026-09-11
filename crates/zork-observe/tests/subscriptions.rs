use futures_util::FutureExt;
use std::sync::{Arc, Barrier};
use zork_observe::{Changes, JournalLimits, List, ListEdit, ResetReason, Source, Topics};

const A: Topics = Topics::new(1);
const B: Topics = Topics::new(2);

#[test]
fn initial_empty_snapshot_and_prepare_are_explicit_and_idempotent() {
    // State need not implement Clone or PartialEq; the reducer owns no-op checks.
    struct State(Vec<usize>);
    let source = Source::new(State(vec![]), JournalLimits::default());
    let mut reader = source.subscribe();
    assert!(reader.ready().now_or_never().unwrap().is_ok());
    let batch = reader.prepare().unwrap();
    assert!(matches!(
        batch.changes,
        Changes::Reset(ResetReason::Initial)
    ));
    assert!(batch.snapshot.value.0.is_empty());
    assert!(Arc::ptr_eq(&batch, &reader.prepare().unwrap()));
    assert_eq!(reader.applied(), None);
    assert!(reader.acknowledge(batch.id));
    assert!(!reader.acknowledge(batch.id));
    assert!(reader.prepare().is_none());
    assert!(reader.ready().now_or_never().is_none());
    source.publish(State(vec![1]), (), A, 0);
    assert!(reader.ready().now_or_never().unwrap().is_ok());
    assert_eq!(reader.prepare().unwrap().snapshot.value.0, [1]);
}

#[test]
fn discard_keeps_the_applied_base_and_later_commit_survives_acknowledgement() {
    let source = Source::new(0, JournalLimits::default());
    let mut reader = source.subscribe();
    reader.ready().now_or_never().unwrap().unwrap();
    let initial = reader.prepare().unwrap();
    reader.acknowledge(initial.id);
    source.publish(1, "one", A, 3);
    reader.ready().now_or_never().unwrap().unwrap();
    let abandoned = reader.prepare().unwrap();
    source.publish(2, "two", A, 3);
    assert!(Arc::ptr_eq(&abandoned, &reader.prepare().unwrap()));
    assert!(
        reader.ready().now_or_never().is_none(),
        "queued another invalidation before application"
    );
    reader.discard(abandoned.id);
    reader.ready().now_or_never().unwrap().unwrap();
    let retried = reader.prepare().unwrap();
    let Changes::Delta { from, records } = &retried.changes else {
        panic!("expected replayable delta")
    };
    assert_eq!(*from, initial.snapshot.cursor);
    assert_eq!(
        records.iter().map(|r| *r.value).collect::<Vec<_>>(),
        ["one", "two"]
    );
    source.publish(3, "three", A, 5);
    reader.acknowledge(retried.id);
    reader.ready().now_or_never().unwrap().unwrap();
    let final_batch = reader.prepare().unwrap();
    let Changes::Delta { from, records } = &final_batch.changes else {
        panic!("expected delta")
    };
    assert_eq!(*from, retried.snapshot.cursor);
    assert_eq!(records.len(), 1);
    assert_eq!(*final_batch.snapshot.value, 3);
}

#[test]
fn topics_are_filtered_before_waking_and_bursts_keep_one_pending_notification() {
    let source = Source::new(
        0,
        JournalLimits {
            commits: 8,
            bytes: 4096,
        },
    );
    let (mut a, _) = source.subscribe_topics(A);
    let (mut b, _) = source.subscribe_topics(B);
    for reader in [&mut a, &mut b] {
        reader.ready().now_or_never().unwrap().unwrap();
        let initial = reader.prepare().unwrap();
        reader.acknowledge(initial.id);
        assert!(reader.ready().now_or_never().is_none());
    }
    for index in 1..=10_000 {
        source.publish(index, index, A, 8);
    }
    a.ready().now_or_never().unwrap().unwrap();
    assert!(a.ready().now_or_never().is_none());
    assert!(b.ready().now_or_never().is_none());
    assert!(b.prepare().is_none());
    let latest = a.prepare().unwrap();
    assert!(matches!(
        latest.changes,
        Changes::Reset(ResetReason::Lagged)
    ));
    assert_eq!(*latest.snapshot.value, 10_000);
    assert!(source.retained().0 <= 8);
    assert!(source.retained().1 <= 4096);
    a.acknowledge(latest.id);
    source.publish(10_001, 10_001, A | B, 8);
    a.ready().now_or_never().unwrap().unwrap();
    b.ready().now_or_never().unwrap().unwrap();
}

#[test]
fn last_committed_state_can_be_read_after_publisher_closes() {
    let source = Source::new(0, JournalLimits::default());
    let mut reader = source.subscribe();
    reader.ready().now_or_never().unwrap().unwrap();
    let initial = reader.prepare().unwrap();
    reader.acknowledge(initial.id);
    source.publish(1, (), A, 0);
    drop(source);
    reader.ready().now_or_never().unwrap().unwrap();
    let final_batch = reader.prepare().unwrap();
    assert_eq!(*final_batch.snapshot.value, 1);
    reader.acknowledge(final_batch.id);
    assert!(reader.ready().now_or_never().unwrap().is_err());
    assert!(reader.prepare().is_none());
}

#[test]
fn reset_and_foreign_batches_cannot_acknowledge_a_new_baseline() {
    let source = Source::<_, ()>::new(0, JournalLimits::default());
    let mut a = source.subscribe();
    let mut b = source.subscribe();
    let first = a.prepare().unwrap();
    assert!(!b.acknowledge(first.id));
    a.reset();
    let second = a.prepare().unwrap();
    assert_ne!(first.id, second.id);
    assert!(!a.acknowledge(first.id));
    assert!(a.acknowledge(second.id));
    source.replace(1);
    assert!(matches!(
        a.prepare().unwrap().changes,
        Changes::Reset(ResetReason::Replaced)
    ));
    let mut readiness = b.readiness();
    readiness.changed().now_or_never().unwrap().unwrap();
    drop(b);
    assert!(readiness.changed().now_or_never().unwrap().is_err());
    drop(a);
    assert!(!source.observed());
}

#[test]
fn a_single_large_patch_also_obeys_the_byte_budget() {
    let source = Source::new(
        0,
        JournalLimits {
            commits: 100,
            bytes: 256,
        },
    );
    let mut reader = source.subscribe();
    let initial = reader.prepare().unwrap();
    reader.acknowledge(initial.id);
    source.publish(1, vec![0; 4096], A, 4096);
    assert_eq!(source.retained(), (0, 0));
    assert!(matches!(
        reader.prepare().unwrap().changes,
        Changes::Reset(ResetReason::Lagged)
    ));
}

#[test]
fn replacement_invalidates_prepared_payloads_before_they_can_advance_the_baseline() {
    let source = Source::<_, ()>::new("old protected content", JournalLimits::default());
    let mut reader = source.subscribe();
    reader.ready().now_or_never().unwrap().unwrap();
    let old = reader.prepare().unwrap();
    source.replace("cleared");
    assert!(!reader.acknowledge(old.id));
    assert_eq!(reader.applied(), None);
    reader.ready().now_or_never().unwrap().unwrap();
    let current = reader.prepare().unwrap();
    assert_eq!(*current.snapshot.value, "cleared");
    assert!(current.is_reset());
    assert!(reader.acknowledge(current.id));
    source.publish("updated", (), A, 0);
    let abandoned = reader.prepare().unwrap();
    source.replace("cleared again");
    let refreshed = reader.prepare().unwrap();
    assert_ne!(abandoned.id, refreshed.id);
    assert_eq!(*refreshed.snapshot.value, "cleared again");
    assert!(!reader.acknowledge(abandoned.id));
    assert!(reader.acknowledge(refreshed.id));
}

#[test]
fn revocation_interrupts_an_already_pending_frame() {
    let source = Source::<_, ()>::new("visible", JournalLimits::default());
    let mut reader = source.subscribe();
    let initial = reader.prepare().unwrap();
    reader.acknowledge(initial.id);
    let mut readiness = reader.readiness();
    assert!(readiness.changed().now_or_never().is_none());
    source.publish("queued frame", (), A, 0);
    readiness.changed().now_or_never().unwrap().unwrap();
    let queued = reader.prepare().unwrap();
    source.invalidate("revoked");
    readiness.changed().now_or_never().unwrap().unwrap();
    assert!(readiness.take_urgent());
    assert!(!readiness.take_urgent());
    let cleared = reader.prepare().unwrap();
    assert_eq!(*cleared.snapshot.value, "revoked");
    assert!(!reader.acknowledge(queued.id));
    assert!(reader.acknowledge(cleared.id));
}

#[test]
fn concurrent_commit_and_acknowledgement_never_lose_the_next_wake() {
    let source = Arc::new(Source::new(0, JournalLimits::default()));
    let mut reader = source.subscribe();
    reader.ready().now_or_never().unwrap().unwrap();
    let initial = reader.prepare().unwrap();
    reader.acknowledge(initial.id);
    for step in 0..100 {
        source.publish(step * 2 + 1, (), A, 0);
        reader.ready().now_or_never().unwrap().unwrap();
        let prepared = reader.prepare().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let writer = {
            let source = source.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                source.publish(step * 2 + 2, (), A, 0);
            })
        };
        barrier.wait();
        reader.acknowledge(prepared.id);
        writer.join().unwrap();
        reader.ready().now_or_never().unwrap().unwrap();
        let current = reader.prepare().unwrap();
        assert_eq!(*current.snapshot.value, step * 2 + 2);
        reader.acknowledge(current.id);
    }
}

#[test]
fn slow_and_cancelled_readers_converge_to_the_same_list_as_fast_readers() {
    let mut state: List<usize> = (0..100).collect();
    let source = Source::new(
        state.clone(),
        JournalLimits {
            commits: 8,
            bytes: 2048,
        },
    );
    let mut readers = (0..8)
        .map(|_| (source.subscribe(), List::new()))
        .collect::<Vec<_>>();
    let mut random = 71_u64;
    for step in 0..1_000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let start = random as usize % (state.len() + 1);
        let end = (start + (random >> 32) as usize % 3).min(state.len());
        let edit = ListEdit {
            remove: start..end,
            insert: (0..step % 3).map(|i| step * 3 + i).collect(),
        };
        assert!(edit.apply(&mut state));
        source.publish(state.clone(), edit, A, 64);
        for (index, (reader, mirror)) in readers.iter_mut().enumerate() {
            if step % (index * 4 + 1) != 0 && step != 999 {
                continue;
            }
            let batch = reader.prepare().unwrap();
            if step % 17 == 0 && step != 999 {
                reader.discard(batch.id);
                continue;
            }
            match &batch.changes {
                Changes::Reset(_) => *mirror = batch.snapshot.value.as_ref().clone(),
                Changes::Delta { records, .. } => {
                    for record in records {
                        assert!(record.value.apply(mirror));
                    }
                }
            }
            assert_eq!(mirror, batch.snapshot.value.as_ref());
            reader.acknowledge(batch.id);
        }
        assert!(source.retained().0 <= 8);
        assert!(source.retained().1 <= 2048);
    }
    for (_, mirror) in readers {
        assert_eq!(mirror, state);
    }
}
