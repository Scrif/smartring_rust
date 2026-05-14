use crate::packet::Packet;

/// Command byte for real-time heart rate readings.
pub const CMD_REAL_TIME_HR: u8 = 0x30;

/// Command byte for real-time SpO2 (blood oxygen) readings.
pub const CMD_REAL_TIME_SPO2: u8 = 0x69;

/// Upper bound on packets collected by `send_recv_until` for real-time commands.
/// The wall-clock timeout in the CLI is the primary stop signal; this is a safety cap.
pub const MAX_REAL_TIME_PACKETS: usize = 300;

// ── Request builders ──────────────────────────────────────────────────────────

/// Start a real-time heart rate measurement.
pub fn real_time_hr_start() -> Packet {
    Packet::new(CMD_REAL_TIME_HR, &[0x00, 0x01])
}

/// Stop the real-time heart rate sensor.
pub fn real_time_hr_stop() -> Packet {
    Packet::new(CMD_REAL_TIME_HR, &[0x00, 0x00])
}

/// Start a real-time SpO2 measurement.
pub fn real_time_spo2_start() -> Packet {
    Packet::new(CMD_REAL_TIME_SPO2, &[0x01])
}

/// Stop the real-time SpO2 sensor.
pub fn real_time_spo2_stop() -> Packet {
    Packet::new(CMD_REAL_TIME_SPO2, &[0x00])
}

// ── Value extractors ──────────────────────────────────────────────────────────

/// Return the heart rate (bpm) from a response packet if it is non-zero.
///
/// Response layout: command=0x30, `subdata[3]` = bpm. Zero means the sensor
/// is still acquiring a reading.
pub fn extract_heart_rate(packet: &Packet) -> Option<u8> {
    if packet.command == CMD_REAL_TIME_HR {
        let bpm = packet.subdata[3];
        if bpm != 0 { Some(bpm) } else { None }
    } else {
        None
    }
}

/// Return the SpO2 percentage from a response packet if it is non-zero.
///
/// Response layout: command=0x69, `subdata[2]` = SpO2 %. Zero means the sensor
/// is still acquiring a reading.
pub fn extract_spo2(packet: &Packet) -> Option<u8> {
    if packet.command == CMD_REAL_TIME_SPO2 {
        let pct = packet.subdata[2];
        if pct != 0 { Some(pct) } else { None }
    } else {
        None
    }
}

// ── Done predicates (for Client::send_recv_until) ────────────────────────────

/// Returns `true` once any collected packet contains a non-zero heart rate.
pub fn heart_rate_done(received: &[Packet]) -> bool {
    received.iter().any(|p| extract_heart_rate(p).is_some())
}

/// Returns `true` once any collected packet contains a non-zero SpO2 value.
pub fn spo2_done(received: &[Packet]) -> bool {
    received.iter().any(|p| extract_spo2(p).is_some())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Request packets ───────────────────────────────────────────────────────

    #[test]
    fn hr_start_uses_command_0x30() {
        assert_eq!(real_time_hr_start().command, CMD_REAL_TIME_HR);
    }

    #[test]
    fn hr_start_subdata_enables_sensor() {
        let pkt = real_time_hr_start();
        assert_eq!(pkt.subdata[0], 0x00);
        assert_eq!(pkt.subdata[1], 0x01);
    }

    #[test]
    fn hr_stop_subdata_disables_sensor() {
        let pkt = real_time_hr_stop();
        assert_eq!(pkt.subdata[0], 0x00);
        assert_eq!(pkt.subdata[1], 0x00);
    }

    #[test]
    fn spo2_start_uses_command_0x69() {
        assert_eq!(real_time_spo2_start().command, CMD_REAL_TIME_SPO2);
    }

    #[test]
    fn spo2_start_subdata_enables_sensor() {
        assert_eq!(real_time_spo2_start().subdata[0], 0x01);
    }

    #[test]
    fn spo2_stop_subdata_disables_sensor() {
        assert_eq!(real_time_spo2_stop().subdata[0], 0x00);
    }

    // ── extract_heart_rate ────────────────────────────────────────────────────

    #[test]
    fn extract_hr_returns_none_for_zero_bpm() {
        // Ring still acquiring — subdata[3] = 0
        let pkt = Packet::new(CMD_REAL_TIME_HR, &[0x00, 0x00, 0x00, 0x00]);
        assert_eq!(extract_heart_rate(&pkt), None);
    }

    #[test]
    fn extract_hr_returns_bpm_when_nonzero() {
        let mut sub = [0u8; 14];
        sub[3] = 72;
        let pkt = Packet::new(CMD_REAL_TIME_HR, &sub);
        assert_eq!(extract_heart_rate(&pkt), Some(72));
    }

    #[test]
    fn extract_hr_ignores_wrong_command() {
        let pkt = Packet::new(CMD_REAL_TIME_SPO2, &[0x00, 0x00, 0x00, 72]);
        assert_eq!(extract_heart_rate(&pkt), None);
    }

    // ── extract_spo2 ──────────────────────────────────────────────────────────

    #[test]
    fn extract_spo2_returns_none_for_zero() {
        let pkt = Packet::new(CMD_REAL_TIME_SPO2, &[0x00, 0x00, 0x00]);
        assert_eq!(extract_spo2(&pkt), None);
    }

    #[test]
    fn extract_spo2_returns_pct_when_nonzero() {
        let mut sub = [0u8; 14];
        sub[2] = 98;
        let pkt = Packet::new(CMD_REAL_TIME_SPO2, &sub);
        assert_eq!(extract_spo2(&pkt), Some(98));
    }

    #[test]
    fn extract_spo2_ignores_wrong_command() {
        let pkt = Packet::new(CMD_REAL_TIME_HR, &[0x00, 0x00, 98]);
        assert_eq!(extract_spo2(&pkt), None);
    }

    // ── done predicates (mock notification stream) ────────────────────────────

    #[test]
    fn heart_rate_done_false_on_empty() {
        assert!(!heart_rate_done(&[]));
    }

    #[test]
    fn heart_rate_done_false_while_all_zero() {
        // Simulates the ring still measuring — multiple zero-bpm packets
        let zero = Packet::new(CMD_REAL_TIME_HR, &[0u8; 14]);
        assert!(!heart_rate_done(&[zero.clone(), zero]));
    }

    #[test]
    fn heart_rate_done_true_on_first_nonzero() {
        // Simulates: two "still measuring" packets, then a valid reading
        let zero = Packet::new(CMD_REAL_TIME_HR, &[0u8; 14]);
        let mut sub = [0u8; 14];
        sub[3] = 72;
        let reading = Packet::new(CMD_REAL_TIME_HR, &sub);
        assert!(heart_rate_done(&[zero.clone(), zero, reading]));
    }

    #[test]
    fn spo2_done_false_on_empty() {
        assert!(!spo2_done(&[]));
    }

    #[test]
    fn spo2_done_false_while_all_zero() {
        let zero = Packet::new(CMD_REAL_TIME_SPO2, &[0u8; 14]);
        assert!(!spo2_done(&[zero.clone(), zero]));
    }

    #[test]
    fn spo2_done_true_on_first_nonzero() {
        let zero = Packet::new(CMD_REAL_TIME_SPO2, &[0u8; 14]);
        let mut sub = [0u8; 14];
        sub[2] = 98;
        let reading = Packet::new(CMD_REAL_TIME_SPO2, &sub);
        assert!(spo2_done(&[zero.clone(), zero, reading]));
    }

    /// Integration test: run manually with a real ring wearing.
    ///
    /// cargo test -p smartring-core -- --ignored real_time_hr_round_trip
    #[tokio::test]
    #[ignore = "requires a worn Colmi ring and Bluetooth adapter"]
    async fn real_time_hr_round_trip() {
        // Placeholder — verified manually via `smartring get-real-time heart-rate`
    }
}
