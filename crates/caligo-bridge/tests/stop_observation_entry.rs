#![cfg(windows)]

use caligo_bridge::{caligo_qq_entry_shutdown_observe, validate_stop_observation_config};

fn pack(duration_ms: u32, wait_ms: u32) -> usize {
    (u64::from(duration_ms) | (u64::from(wait_ms) << 32)) as usize
}

#[test]
fn timing_contract_accepts_endpoints_and_rejects_out_of_range_values() {
    for (duration_ms, wait_ms) in [(1, 251), (5_000, 5_250), (5_000, 30_000)] {
        assert!(validate_stop_observation_config(duration_ms, wait_ms));
    }
    for (duration_ms, wait_ms) in [
        (0, 251),
        (1, 250),
        (5_000, 5_249),
        (5_001, 5_251),
        (5_000, 30_001),
        (u32::MAX, u32::MAX),
    ] {
        assert!(!validate_stop_observation_config(duration_ms, wait_ms));
    }
}

#[cfg(not(feature = "research"))]
#[test]
fn default_export_rejects_every_packed_value() {
    for packed in [0, usize::MAX, pack(1, 251), pack(5_000, 30_000)] {
        assert_eq!(
            caligo_qq_entry_shutdown_observe(packed),
            caligo_bridge::gate::ERR_RESEARCH_DISABLED
        );
    }
}

#[cfg(feature = "research")]
#[test]
fn invalid_packed_values_are_rejected_before_startup_lookup() {
    for packed in [
        0,
        usize::MAX,
        pack(0, 251),
        pack(251, 1),
        pack(1, 250),
        pack(5_001, 5_251),
        pack(5_000, 30_001),
    ] {
        assert_eq!(
            caligo_qq_entry_shutdown_observe(packed),
            caligo_bridge::qq_entry_code::ERR_STOP_OBSERVATION_FAILED
        );
    }
}

#[cfg(feature = "research")]
#[test]
fn valid_packed_values_require_formal_daemon_startup() {
    for packed in [pack(1, 251), pack(5_000, 30_000)] {
        assert_eq!(
            caligo_qq_entry_shutdown_observe(packed),
            caligo_bridge::qq_entry_code::ERR_SHUTDOWN_NOT_READY
        );
    }
}
