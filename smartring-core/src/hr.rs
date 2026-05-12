use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use thiserror::Error;

use crate::packet::Packet;

const CMD_HR_LOG: u8 = 0x15;

/// Number of 10-minute slots in 24 hours (24 × 6 = 144).
pub const READINGS_PER_DAY: usize = 144;

/// Number of response packets per heart-rate log request (12 packets × 12 readings = 144).
///
/// NOTE: verify against real hardware — ring models may differ.
pub const HR_LOG_PACKETS: usize = 12;

/// Readings per response packet (subdata[1..13]).
const READINGS_PER_PACKET: usize = 12;

#[derive(Debug, Error, PartialEq)]
pub enum HrError {
    #[error("unexpected command byte: expected {expected:#04x}, got {got:#04x}")]
    WrongCommand { expected: u8, got: u8 },
    #[error("packet index {0} out of range (max {1})")]
    PacketIndexOutOfRange(u8, usize),
}

/// A full day's worth of heart rate readings assembled from ring response packets.
#[derive(Debug, Clone)]
pub struct HeartRateLog {
    /// Midnight UTC of the day these readings cover.
    pub base_time: DateTime<Utc>,
    /// 144 readings, one per 10-minute slot. 0 = no measurement recorded.
    pub readings: Vec<u8>,
}

impl HeartRateLog {
    /// Returns `(timestamp, bpm)` pairs for every non-zero reading.
    ///
    /// Timestamps start at midnight UTC (`base_time`) and advance 10 minutes per slot.
    /// Zero-valued slots (no measurement) are excluded.
    pub fn readings_with_times(&self) -> Vec<(DateTime<Utc>, u8)> {
        self.readings
            .iter()
            .enumerate()
            .filter(|(_, &bpm)| bpm != 0)
            .map(|(i, &bpm)| (self.base_time + Duration::minutes(i as i64 * 10), bpm))
            .collect()
    }
}

/// Build a heart-rate log request packet for the given date.
///
/// Subdata layout: `[year_low, year_high, month, day]` — same year encoding as set-time.
pub fn heart_rate_log_request(date: NaiveDate) -> Packet {
    let year = date.year() as u16;
    Packet::new(
        CMD_HR_LOG,
        &[
            (year & 0xFF) as u8,
            (year >> 8) as u8,
            date.month() as u8,
            date.day() as u8,
        ],
    )
}

/// Assemble a [`HeartRateLog`] from the ring's multi-packet response.
///
/// Each response packet layout (NOTE: verify byte positions with real hardware):
///   `subdata[0]`    — packet index (0–11)
///   `subdata[1..13]` — 12 HR readings for the corresponding 10-minute slots
///   `subdata[13]`   — unused / zero
pub fn parse_heart_rate_log(date: NaiveDate, packets: &[Packet]) -> Result<HeartRateLog, HrError> {
    let base_time = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let mut readings = vec![0u8; READINGS_PER_DAY];

    for packet in packets {
        if packet.command != CMD_HR_LOG {
            return Err(HrError::WrongCommand {
                expected: CMD_HR_LOG,
                got: packet.command,
            });
        }

        let pkt_idx = packet.subdata[0] as usize;
        let start = pkt_idx * READINGS_PER_PACKET;
        if start >= READINGS_PER_DAY {
            return Err(HrError::PacketIndexOutOfRange(
                packet.subdata[0],
                HR_LOG_PACKETS - 1,
            ));
        }
        let end = (start + READINGS_PER_PACKET).min(READINGS_PER_DAY);
        readings[start..end].copy_from_slice(&packet.subdata[1..1 + (end - start)]);
    }

    Ok(HeartRateLog { base_time, readings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 10).unwrap()
    }

    // ── request encoding ──────────────────────────────────────────────────

    #[test]
    fn request_uses_command_0x15() {
        assert_eq!(heart_rate_log_request(date()).command, 0x15);
    }

    #[test]
    fn request_encodes_date_correctly() {
        let pkt = heart_rate_log_request(date());
        // 2026 = 0x07EA: low=0xEA=234, high=0x07=7
        assert_eq!(pkt.subdata[0], 0xEA, "year low");
        assert_eq!(pkt.subdata[1], 0x07, "year high");
        assert_eq!(pkt.subdata[2], 5, "month");
        assert_eq!(pkt.subdata[3], 10, "day");
    }

    // ── HeartRateLog::readings_with_times ────────────────────────────────

    #[test]
    fn readings_with_times_excludes_zeros() {
        let base_time = date().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let mut readings = vec![0u8; READINGS_PER_DAY];
        readings[1] = 72; // slot 1 → 00:10
        readings[3] = 75; // slot 3 → 00:30

        let log = HeartRateLog { base_time, readings };
        let pairs = log.readings_with_times();

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].1, 72);
        assert_eq!(pairs[1].1, 75);
    }

    #[test]
    fn readings_with_times_all_zero_returns_empty() {
        let base_time = date().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let log = HeartRateLog { base_time, readings: vec![0u8; READINGS_PER_DAY] };
        assert!(log.readings_with_times().is_empty());
    }

    #[test]
    fn readings_with_times_correct_timestamps() {
        let base_time = date().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let mut readings = vec![0u8; READINGS_PER_DAY];
        readings[0] = 70;   // slot 0  → 00:00
        readings[6] = 75;   // slot 6  → 01:00
        readings[143] = 80; // slot 143 → 23:50

        let log = HeartRateLog { base_time, readings };
        let pairs = log.readings_with_times();

        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].0, base_time);
        assert_eq!(pairs[1].0, base_time + Duration::hours(1));
        assert_eq!(pairs[2].0, base_time + Duration::minutes(143 * 10));
        assert_eq!(pairs[2].0.to_rfc3339(), "2026-05-10T23:50:00+00:00");
    }

    // ── parse_heart_rate_log ─────────────────────────────────────────────

    /// Synthetic "known capture": 12 packets, first has two non-zero readings.
    fn make_capture(slot0: u8, slot2: u8) -> Vec<Packet> {
        let mut packets = Vec::new();
        // Packet 0: slots 0–11; only slots 0 and 2 have readings.
        let mut sub0 = [0u8; 14];
        sub0[0] = 0; // packet index
        sub0[1] = slot0; // slot 0 = 00:00
        sub0[3] = slot2; // slot 2 = 00:20
        packets.push(Packet::new(0x15, &sub0));
        // Packets 1–11: all zero readings.
        for i in 1..HR_LOG_PACKETS {
            let mut sub = [0u8; 14];
            sub[0] = i as u8;
            packets.push(Packet::new(0x15, &sub));
        }
        packets
    }

    #[test]
    fn parse_from_known_capture() {
        let packets = make_capture(72, 75);
        let log = parse_heart_rate_log(date(), &packets).unwrap();

        assert_eq!(log.readings.len(), READINGS_PER_DAY);
        assert_eq!(log.readings[0], 72);
        assert_eq!(log.readings[1], 0);
        assert_eq!(log.readings[2], 75);

        let pairs = log.readings_with_times();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].1, 72);
        assert_eq!(pairs[0].0.to_rfc3339(), "2026-05-10T00:00:00+00:00");
        assert_eq!(pairs[1].1, 75);
        assert_eq!(pairs[1].0.to_rfc3339(), "2026-05-10T00:20:00+00:00");
    }

    #[test]
    fn parse_empty_packet_list_returns_all_zeros() {
        let log = parse_heart_rate_log(date(), &[]).unwrap();
        assert_eq!(log.readings, vec![0u8; READINGS_PER_DAY]);
        assert!(log.readings_with_times().is_empty());
    }

    #[test]
    fn parse_rejects_wrong_command() {
        let bad_pkt = Packet::new(0x03, &[0u8; 14]);
        let err = parse_heart_rate_log(date(), &[bad_pkt]).unwrap_err();
        assert_eq!(err, HrError::WrongCommand { expected: 0x15, got: 0x03 });
    }

    #[test]
    fn parse_rejects_out_of_range_packet_index() {
        let mut sub = [0u8; 14];
        sub[0] = 12; // valid indices are 0–11
        let bad_pkt = Packet::new(0x15, &sub);
        assert!(matches!(
            parse_heart_rate_log(date(), &[bad_pkt]).unwrap_err(),
            HrError::PacketIndexOutOfRange(12, _)
        ));
    }
}
