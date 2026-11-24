#![cfg(feature = "research")]
use caligo_bridge::stop_observation::*;

fn identity() -> StopIdentity {
    StopIdentity {
        account: "10001".into(),
        host_nonce: 7,
        wrapper_session: "nt_fixture".into(),
        listener_generation: 1,
    }
}
fn window(duration: u64, wait: u64) -> StopObservationWindow {
    StopObservationWindow::new(identity(), 0, duration, wait).unwrap()
}
fn next(w: &mut StopObservationWindow, now: u64, kind: ActionKind) -> Action {
    let action = w.next_action(now, &identity()).unwrap().unwrap();
    assert_eq!(action.kind, kind);
    action
}
fn ack(w: &mut StopObservationWindow, now: u64, a: Action, r: Receipt) {
    w.accept_receipt(now, &identity(), a, r).unwrap();
}
fn observed() -> Receipt {
    Receipt::Observed(Sample { callbacks_seen: 3 })
}
fn start(w: &mut StopObservationWindow) {
    let stop = next(w, 0, ActionKind::StopListener);
    ack(w, 0, stop, Receipt::ListenerStopped);
    assert_eq!(w.phase(), Phase::InitialObservation);
    let initial = next(
        w,
        0,
        ActionKind::Observe {
            final_sample: false,
        },
    );
    ack(w, 0, initial, observed());
    assert_eq!(w.phase(), Phase::Observing);
}
fn finish(w: &mut StopObservationWindow, now: u64) {
    let final_sample = next(w, now, ActionKind::Observe { final_sample: true });
    assert_eq!(w.phase(), Phase::FinalObservation);
    assert!(w.next_action(now, &identity()).unwrap().is_none());
    ack(w, now, final_sample, observed());
    assert_eq!(w.phase(), Phase::ReadyToClose);
    let close = next(w, now, ActionKind::ClosePump);
    assert!(w.next_action(now, &identity()).unwrap().is_none());
    ack(w, now, close, Receipt::PumpClosed);
    assert_eq!(w.phase(), Phase::Closed);
    assert!(w.next_action(now, &identity()).unwrap().is_none());
}
fn quarantined(w: &mut StopObservationWindow, now: u64, failure: Failure) {
    assert_eq!(w.phase(), Phase::Quarantined(failure));
    assert_eq!(w.next_action(now, &identity()), Err(failure));
}

#[test]
fn validates_duration_deadline_identity_and_overflow() {
    for (duration, wait) in [(0, 100), (5001, 6000), (100, 99)] {
        assert!(matches!(
            StopObservationWindow::new(identity(), 0, duration, wait),
            Err(Failure::InvalidConfig)
        ));
    }
    assert!(StopObservationWindow::new(identity(), u64::MAX, 1, 1).is_err());
    let mut invalid = vec![];
    let mut i = identity();
    i.host_nonce = 0;
    invalid.push(i);
    let mut i = identity();
    i.wrapper_session = "x".repeat(129);
    invalid.push(i);
    let mut i = identity();
    i.wrapper_session.clear();
    invalid.push(i);
    let mut i = identity();
    i.account.clear();
    invalid.push(i);
    let mut i = identity();
    i.listener_generation = 0;
    invalid.push(i);
    let mut i = identity();
    i.listener_generation = i32::MAX as u32 + 1;
    invalid.push(i);
    for i in invalid {
        assert!(StopObservationWindow::new(i, 0, 1, 1).is_err());
    }
}

#[test]
fn max_window_is_bounded_with_no_poll_or_stop_replay() {
    let mut w = window(5000, 5000);
    let stop = next(&mut w, 0, ActionKind::StopListener);
    for _ in 0..128 {
        assert!(w.next_action(0, &identity()).unwrap().is_none());
    }
    ack(&mut w, 0, stop, Receipt::ListenerStopped);
    let initial = next(
        &mut w,
        0,
        ActionKind::Observe {
            final_sample: false,
        },
    );
    ack(&mut w, 0, initial, observed());
    for now in 1..5000 {
        let action = w.next_action(now, &identity()).unwrap();
        if now % SAMPLE_INTERVAL_MS == 0 {
            let action = action.unwrap();
            assert_eq!(
                action.kind,
                ActionKind::Observe {
                    final_sample: false
                }
            );
            ack(&mut w, now, action, observed());
        } else {
            assert!(action.is_none());
        }
    }
    finish(&mut w, 5000);
    assert_eq!(w.actions_issued(), 103);
    assert!(w.actions_issued() <= MAX_ACTIONS && MAX_ACTIONS <= 128);
    assert_eq!(w.last_sample(), Some(Sample { callbacks_seen: 3 }));
}

#[test]
fn minimum_duration_and_delayed_tick_take_final_once() {
    let mut w = window(1, 100);
    start(&mut w);
    finish(&mut w, 17);
    assert_eq!(w.actions_issued(), 4);
}

#[test]
fn all_identity_drift_quarantines_before_any_action() {
    for field in 0..4 {
        for stopped in [false, true] {
            let mut w = window(100, 200);
            if stopped {
                start(&mut w);
            }
            let mut i = identity();
            match field {
                0 => i.account.push('2'),
                1 => i.host_nonce += 1,
                2 => i.wrapper_session.push('2'),
                _ => i.listener_generation += 1,
            }
            assert_eq!(w.next_action(0, &i), Err(Failure::IdentityMismatch));
            quarantined(&mut w, 0, Failure::IdentityMismatch);
        }
    }
}

#[test]
fn reversal_timeout_and_insufficient_remaining_window_are_terminal() {
    let mut w = window(100, 200);
    start(&mut w);
    assert!(w.next_action(20, &identity()).unwrap().is_none());
    assert_eq!(w.next_action(19, &identity()), Err(Failure::ClockReversed));
    quarantined(&mut w, 20, Failure::ClockReversed);
    let mut w = window(100, 100);
    start(&mut w);
    assert_eq!(w.next_action(101, &identity()), Err(Failure::Timeout));
    quarantined(&mut w, 101, Failure::Timeout);
    let mut w = window(100, 100);
    let stop = next(&mut w, 0, ActionKind::StopListener);
    ack(&mut w, 1, stop, Receipt::ListenerStopped);
    let initial = next(
        &mut w,
        1,
        ActionKind::Observe {
            final_sample: false,
        },
    );
    assert_eq!(
        w.accept_receipt(1, &identity(), initial, observed()),
        Err(Failure::Timeout)
    );
}

#[test]
fn unknown_stop_sample_and_close_never_retry() {
    for stage in 0..3 {
        let mut w = window(100, 200);
        let action = if stage == 0 {
            next(&mut w, 0, ActionKind::StopListener)
        } else {
            start(&mut w);
            let a = next(&mut w, 100, ActionKind::Observe { final_sample: true });
            if stage == 1 {
                a
            } else {
                ack(&mut w, 100, a, observed());
                next(&mut w, 100, ActionKind::ClosePump)
            }
        };
        let now = if stage == 0 { 0 } else { 100 };
        assert_eq!(
            w.accept_receipt(now, &identity(), action, Receipt::Unknown),
            Err(Failure::UnknownOutcome)
        );
        quarantined(&mut w, now, Failure::UnknownOutcome);
    }
}

#[test]
fn forged_duplicate_wrong_kind_and_late_receipts_quarantine() {
    for mode in 0..4 {
        let mut w = window(100, 100);
        let mut a = next(&mut w, 0, ActionKind::StopListener);
        let (now, receipt, expected) = match mode {
            0 => {
                a.serial += 1;
                (0, Receipt::ListenerStopped, Failure::InvalidReceipt)
            }
            1 => (0, observed(), Failure::InvalidReceipt),
            2 => {
                ack(&mut w, 0, a, Receipt::ListenerStopped);
                (0, Receipt::ListenerStopped, Failure::InvalidReceipt)
            }
            _ => (101, Receipt::ListenerStopped, Failure::Timeout),
        };
        assert_eq!(
            w.accept_receipt(now, &identity(), a, receipt),
            Err(expected)
        );
        quarantined(&mut w, now, expected);
    }
}

#[test]
fn cancellation_before_stop_and_while_stop_pending() {
    let mut w = window(100, 200);
    w.cancel(0, &identity()).unwrap();
    assert_eq!(w.phase(), Phase::Cancelled);
    assert!(w.next_action(0, &identity()).unwrap().is_none());
    let mut w = window(100, 200);
    let stop = next(&mut w, 0, ActionKind::StopListener);
    w.cancel(0, &identity()).unwrap();
    assert!(w.next_action(0, &identity()).unwrap().is_none());
    ack(&mut w, 0, stop, Receipt::ListenerStopped);
    let initial = next(
        &mut w,
        0,
        ActionKind::Observe {
            final_sample: false,
        },
    );
    ack(&mut w, 0, initial, observed());
    finish(&mut w, 0);
}

#[test]
fn cancellation_after_stop_requires_final_receipt_even_with_sample_pending() {
    let mut w = window(100, 200);
    start(&mut w);
    let intermediate = next(
        &mut w,
        50,
        ActionKind::Observe {
            final_sample: false,
        },
    );
    w.cancel(50, &identity()).unwrap();
    assert!(w.next_action(50, &identity()).unwrap().is_none());
    ack(&mut w, 50, intermediate, observed());
    finish(&mut w, 50);
}
