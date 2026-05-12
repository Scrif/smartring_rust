use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use thiserror::Error;

use crate::packet::Packet;

pub const CMD_STEPS: u8 = 0x43;

const NO_DATA_BYTE: u8 = 0xFF;
const HEADER_BYTE: u8 = 0xF0;

/// Upper bound on packets to collect; prevents runaway loops on a misbehaving ring.
pub const MAX_SPORT_PACKETS: usize = 50;

/// Build a sport-detail request for the given day offset.
///
/// `day_offset = 0` → today (relative to the ring's internal clock);
/// `day_offset = 1` → yesterday, and so on.
pub fn sport_detail_request(day_offset: u8) -> Packet {
    Packet::new(CMD_STEPS, &[day_offset, 0x0F, 0x00, 0x5F, 0x01])
}

/// Returns `true` when the accumulated sport-detail packets indicate all data has arrived.
///
/// Pass this predicate to [`Client::send_recv_until`] to terminate collection automatically.
pub fn sport_detail_done(received: &[Packet]) -> bool {
    match received.last() {
        None => false,
        Some(p) if p.command != CMD_STEPS => false,
        Some(p) => {
            let b0 = p.subdata[0];
            if b0 == NO_DATA_BYTE {
                return true; // ring has no data → done immediately
            }
            if b0 == HEADER_BYTE {
                return false; // header → keep reading
            }
            // Data packet: done when current sequence == total - 1.
            let seq = p.subdata[4];
            let total = p.subdata[5];
            total > 0 && seq == total - 1
        }
    }
}

/// A single 15-minute sport-detail interval returned by the ring.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SportDetail {
    /// Start of this activity interval (UTC).
    pub timestamp: DateTime<Utc>,
    pub steps: u32,
    pub calories: u32,
    /// Distance in metres.
    pub distance: u32,
}

/// Result of a sport-detail query.
#[derive(Debug, PartialEq)]
pub enum StepsResult {
    Data(Vec<SportDetail>),
    NoData,
}

#[derive(Debug, Error, PartialEq)]
pub enum StepsError {
    #[error("unexpected command byte: expected {expected:#04x}, got {got:#04x}")]
    WrongCommand { expected: u8, got: u8 },
    #[error("invalid date in packet: {year}-{month:02}-{day:02} time_index={time_index}")]
    InvalidDate { year: i32, month: u32, day: u32, time_index: u8 },
}

/// Decode a BCD-encoded byte to its decimal value.
///
/// Example: `0x23` → `23`.
fn bcd_to_decimal(b: u8) -> u8 {
    ((b >> 4) & 0xF) * 10 + (b & 0xF)
}

/// Assemble [`SportDetail`] records from the ring's multi-packet response.
///
/// Packet layout (all indices into `Packet.subdata`, i.e. wire bytes offset by −1):
///
/// | `subdata[0]` | Meaning |
/// |---|---|
/// | `0xFF` | No data for this day → returns [`StepsResult::NoData`] |
/// | `0xF0` | Header: `subdata[2] == 1` enables new-calorie protocol (×10) |
/// | other | Data: BCD year (`+2000`), month, day; `time_index`; seq; total; calories; steps; distance |
///
/// Data packet byte map:
/// ```text
/// subdata[0..2]  BCD (year, month, day)
/// subdata[3]     time_index  (15-min slots from midnight; hour = index/4, min = (index%4)*15)
/// subdata[4]     sequence    (0-based, data packets only)
/// subdata[5]     total       (number of data packets)
/// subdata[6..7]  calories    (LE u16; ×10 if new-calorie protocol)
/// subdata[8..9]  steps       (LE u16)
/// subdata[10..11] distance   (LE u16, metres)
/// ```
pub fn parse_sport_details(packets: &[Packet]) -> Result<StepsResult, StepsError> {
    if packets.is_empty() {
        return Ok(StepsResult::NoData);
    }

    let mut new_calorie_protocol = false;
    let mut details = Vec::new();

    for packet in packets {
        if packet.command != CMD_STEPS {
            return Err(StepsError::WrongCommand { expected: CMD_STEPS, got: packet.command });
        }

        let b0 = packet.subdata[0];

        if b0 == NO_DATA_BYTE {
            return Ok(StepsResult::NoData);
        }

        if b0 == HEADER_BYTE {
            new_calorie_protocol = packet.subdata[2] == 1;
            continue;
        }

        // Data packet
        let year = bcd_to_decimal(b0) as i32 + 2000;
        let month = bcd_to_decimal(packet.subdata[1]) as u32;
        let day = bcd_to_decimal(packet.subdata[2]) as u32;
        let time_index = packet.subdata[3];
        let hour = (time_index / 4) as u32;
        let minute = ((time_index % 4) * 15) as u32;

        let ts = NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|d| d.and_hms_opt(hour, minute, 0))
            .ok_or(StepsError::InvalidDate { year, month, day, time_index })?
            .and_utc();

        let mut calories = u16::from_le_bytes([packet.subdata[6], packet.subdata[7]]) as u32;
        if new_calorie_protocol {
            calories *= 10;
        }
        let steps = u16::from_le_bytes([packet.subdata[8], packet.subdata[9]]) as u32;
        let distance = u16::from_le_bytes([packet.subdata[10], packet.subdata[11]]) as u32;

        details.push(SportDetail { timestamp: ts, steps, calories, distance });
    }

    if details.is_empty() {
        Ok(StepsResult::NoData)
    } else {
        Ok(StepsResult::Data(details))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Build a header packet (subdata[0] == 0xF0).
    /// `new_calorie` sets subdata[2] = 1 to enable the ×10 calorie protocol.
    fn header_packet(new_calorie: bool) -> Packet {
        let mut sub = [0u8; 14];
        sub[0] = HEADER_BYTE;
        if new_calorie {
            sub[2] = 1;
        }
        Packet::new(CMD_STEPS, &sub)
    }

    /// Build a NoData sentinel packet.
    fn no_data_packet() -> Packet {
        let mut sub = [0u8; 14];
        sub[0] = NO_DATA_BYTE;
        Packet::new(CMD_STEPS, &sub)
    }

    /// Build a data packet matching the Python example capture.
    ///
    /// Example data date: BCD(0x23)=23 → 2023, BCD(0x08)=8, BCD(0x13)=13 → 2023-08-13.
    ///
    /// - Packet 0 (header, new_calorie=true)
    /// - Packets 1–5 (seq 0–4, total=5)
    fn example_capture() -> Vec<Packet> {
        let mut packets = vec![header_packet(true)];

        // seq=0: 04:00, calories=200*10=2000, steps=48, distance=27
        // BCD(0x23)=23 → 2023, BCD(0x08)=8, BCD(0x13)=13 → 2023-08-13
        packets.push(Packet::new(CMD_STEPS, &[
            0x23, 0x08, 0x13, // year=2023(BCD), month=8(BCD), day=13(BCD)
            0x10,             // time_index=16 → 04:00
            0x00, 0x05,       // seq=0, total=5
            0xC8, 0x00,       // calories=200
            0x30, 0x00,       // steps=48
            0x1B, 0x00,       // distance=27
            0x00, 0x00,
        ]));

        // seq=1: 05:00, calories=6326*10=63260, steps=1194, distance=873
        packets.push(Packet::new(CMD_STEPS, &[
            0x23, 0x08, 0x13,
            0x14,             // time_index=20 → 05:00
            0x01, 0x05,
            0xB6, 0x18,       // calories=0x18B6=6326
            0xAA, 0x04,       // steps=0x04AA=1194
            0x69, 0x03,       // distance=0x0369=873
            0x00, 0x00,
        ]));

        // seq=2: 06:00, calories=1080*10=10800, steps=225, distance=149
        packets.push(Packet::new(CMD_STEPS, &[
            0x23, 0x08, 0x13,
            0x18,             // time_index=24 → 06:00
            0x02, 0x05,
            0x38, 0x04,       // calories=0x0438=1080
            0xE1, 0x00,       // steps=225
            0x95, 0x00,       // distance=149
            0x00, 0x00,
        ]));

        // seq=3: 07:00, calories=517*10=5170, steps=108, distance=72
        packets.push(Packet::new(CMD_STEPS, &[
            0x23, 0x08, 0x13,
            0x1C,             // time_index=28 → 07:00
            0x03, 0x05,
            0x05, 0x02,       // calories=0x0205=517
            0x6C, 0x00,       // steps=108
            0x48, 0x00,       // distance=72
            0x00, 0x00,
        ]));

        // seq=4 (last): 19:00, calories=495*10=4950, steps=99, distance=68
        packets.push(Packet::new(CMD_STEPS, &[
            0x23, 0x08, 0x13,
            0x4C,             // time_index=76 → 19:00
            0x04, 0x05,       // seq=4, total=5 → LAST
            0xEF, 0x01,       // calories=0x01EF=495
            0x63, 0x00,       // steps=99
            0x44, 0x00,       // distance=68
            0x00, 0x00,
        ]));

        packets
    }

    // ── sport_detail_request ─────────────────────────────────────────────────

    #[test]
    fn request_uses_command_0x43() {
        assert_eq!(sport_detail_request(0).command, 0x43);
    }

    #[test]
    fn request_encodes_day_offset_and_constants() {
        let pkt = sport_detail_request(2);
        assert_eq!(pkt.subdata[0], 2);
        assert_eq!(pkt.subdata[1], 0x0F);
        assert_eq!(pkt.subdata[2], 0x00);
        assert_eq!(pkt.subdata[3], 0x5F);
        assert_eq!(pkt.subdata[4], 0x01);
    }

    // ── sport_detail_done ────────────────────────────────────────────────────

    #[test]
    fn done_false_on_empty() {
        assert!(!sport_detail_done(&[]));
    }

    #[test]
    fn done_true_on_no_data_packet() {
        assert!(sport_detail_done(&[no_data_packet()]));
    }

    #[test]
    fn done_false_on_header_only() {
        assert!(!sport_detail_done(&[header_packet(false)]));
    }

    #[test]
    fn done_false_on_non_last_data_packet() {
        // seq=0, total=5 → not last
        let pkt = Packet::new(CMD_STEPS, &[0x23, 0x08, 0x13, 0x10, 0x00, 0x05, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(!sport_detail_done(&[header_packet(false), pkt]));
    }

    #[test]
    fn done_true_on_last_data_packet() {
        // seq=4, total=5 → last
        let pkt = Packet::new(CMD_STEPS, &[0x23, 0x08, 0x13, 0x10, 0x04, 0x05, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(sport_detail_done(&[header_packet(false), pkt]));
    }

    #[test]
    fn done_false_for_wrong_command() {
        // Packet from a different command should not trigger done
        let other = Packet::new(0x15, &[0x04, 0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(!sport_detail_done(&[other]));
    }

    // ── bcd_to_decimal ───────────────────────────────────────────────────────

    #[test]
    fn bcd_decodes_correctly() {
        assert_eq!(bcd_to_decimal(0x23), 23); // 2*10+3
        assert_eq!(bcd_to_decimal(0x08), 8);  // 0*10+8
        assert_eq!(bcd_to_decimal(0x13), 13); // 1*10+3  (not the decimal 0x13=19)
        assert_eq!(bcd_to_decimal(0x19), 19); // 1*10+9
        assert_eq!(bcd_to_decimal(0x00), 0);
        assert_eq!(bcd_to_decimal(0x99), 99);
    }

    // ── parse_sport_details ──────────────────────────────────────────────────

    #[test]
    fn parse_empty_returns_no_data() {
        assert_eq!(parse_sport_details(&[]), Ok(StepsResult::NoData));
    }

    #[test]
    fn parse_no_data_sentinel_returns_no_data() {
        assert_eq!(parse_sport_details(&[no_data_packet()]), Ok(StepsResult::NoData));
    }

    #[test]
    fn parse_header_only_returns_no_data() {
        assert_eq!(parse_sport_details(&[header_packet(false)]), Ok(StepsResult::NoData));
    }

    #[test]
    fn parse_rejects_wrong_command() {
        let bad = Packet::new(0x15, &[0u8; 14]);
        let err = parse_sport_details(&[bad]).unwrap_err();
        assert_eq!(err, StepsError::WrongCommand { expected: CMD_STEPS, got: 0x15 });
    }

    #[test]
    fn parse_from_known_capture() {
        let packets = example_capture();
        let result = parse_sport_details(&packets).unwrap();

        let details = match result {
            StepsResult::Data(d) => d,
            StepsResult::NoData => panic!("expected Data, got NoData"),
        };

        assert_eq!(details.len(), 5);

        // First interval: 2023-08-13 04:00 UTC, new_calorie × 10
        // (BCD 0x13 = 13, BCD 0x08 = 8, BCD 0x23 = 23 → 2023-08-13)
        assert_eq!(details[0].timestamp.to_rfc3339(), "2023-08-13T04:00:00+00:00");
        assert_eq!(details[0].steps, 48);
        assert_eq!(details[0].calories, 2000); // 200 × 10
        assert_eq!(details[0].distance, 27);

        // Second interval
        assert_eq!(details[1].timestamp.to_rfc3339(), "2023-08-13T05:00:00+00:00");
        assert_eq!(details[1].steps, 1194);
        assert_eq!(details[1].calories, 63260); // 6326 × 10
        assert_eq!(details[1].distance, 873);

        // Last interval: 19:00
        assert_eq!(details[4].timestamp.to_rfc3339(), "2023-08-13T19:00:00+00:00");
        assert_eq!(details[4].steps, 99);
        assert_eq!(details[4].calories, 4950); // 495 × 10
        assert_eq!(details[4].distance, 68);
    }

    #[test]
    fn parse_old_calorie_protocol_no_multiplication() {
        let mut packets = vec![header_packet(false)]; // new_calorie = false
        packets.push(Packet::new(CMD_STEPS, &[
            0x23, 0x08, 0x13, 0x10, 0x00, 0x01,
            0xC8, 0x00, // calories = 200 (no ×10)
            0x30, 0x00,
            0x1B, 0x00,
            0x00, 0x00,
        ]));

        let result = parse_sport_details(&packets).unwrap();
        let details = match result {
            StepsResult::Data(d) => d,
            StepsResult::NoData => panic!("expected Data"),
        };
        assert_eq!(details[0].calories, 200); // unchanged
    }

    #[test]
    fn parse_time_index_boundaries() {
        // time_index = 0 → 00:00
        // time_index = 95 → 23:45 (last slot of day)
        let make_pkt = |time_index: u8| {
            Packet::new(CMD_STEPS, &[
                0x23, 0x08, 0x13, time_index, 0x00, 0x01,
                0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
            ])
        };

        let pkts = [header_packet(false), make_pkt(0)];
        let result = parse_sport_details(&pkts).unwrap();
        let d = match result { StepsResult::Data(d) => d, _ => panic!() };
        assert_eq!(d[0].timestamp.to_rfc3339(), "2023-08-13T00:00:00+00:00");

        let pkts = [header_packet(false), make_pkt(95)];
        let result = parse_sport_details(&pkts).unwrap();
        let d = match result { StepsResult::Data(d) => d, _ => panic!() };
        assert_eq!(d[0].timestamp.to_rfc3339(), "2023-08-13T23:45:00+00:00");
    }
}
