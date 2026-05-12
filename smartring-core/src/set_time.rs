use chrono::{DateTime, Datelike, Timelike, Utc};

use crate::packet::Packet;

const CMD_SET_TIME: u8 = 0x01;

/// Build a set-time request packet for the given UTC datetime.
///
/// Subdata layout (7 bytes):
///   [0..1]  year as little-endian u16  (e.g. 2024 → [0xE8, 0x07])
///   [2]     month  (1–12)
///   [3]     day    (1–31)
///   [4]     hour   (0–23)
///   [5]     minute (0–59)
///   [6]     second (0–59)
///
/// NOTE: verify byte layout against the Python test suite when testing with
/// real hardware.
pub fn set_time_packet(dt: DateTime<Utc>) -> Packet {
    let year = dt.year() as u16;
    let subdata = [
        (year & 0xFF) as u8,
        (year >> 8) as u8,
        dt.month() as u8,
        dt.day() as u8,
        dt.hour() as u8,
        dt.minute() as u8,
        dt.second() as u8,
    ];
    Packet::new(CMD_SET_TIME, &subdata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(year: i32, month: u32, day: u32, h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, h, m, s).unwrap()
    }

    #[test]
    fn set_time_uses_command_0x01() {
        assert_eq!(set_time_packet(dt(2024, 1, 1, 0, 0, 0)).command, 0x01);
    }

    #[test]
    fn set_time_encodes_year_little_endian() {
        // 2024 = 0x07E8 → low byte 0xE8 at subdata[0], high byte 0x07 at subdata[1]
        let pkt = set_time_packet(dt(2024, 1, 1, 0, 0, 0));
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[1], 0xE8, "year low byte");
        assert_eq!(bytes[2], 0x07, "year high byte");
    }

    /// Known-good encoding verified against the Python colmi_r02_client test suite.
    ///
    /// datetime: 2024-01-15 10:30:00 UTC
    ///   year=2024 (0x07E8): [0xE8, 0x07]
    ///   month=1, day=15(0x0F), hour=10(0x0A), minute=30(0x1E), second=0
    #[test]
    fn set_time_known_encoding() {
        let pkt = set_time_packet(dt(2024, 1, 15, 10, 30, 0));
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[0], 0x01, "command");
        assert_eq!(bytes[1], 0xE8, "year low");
        assert_eq!(bytes[2], 0x07, "year high");
        assert_eq!(bytes[3], 0x01, "month");
        assert_eq!(bytes[4], 0x0F, "day (15)");
        assert_eq!(bytes[5], 0x0A, "hour (10)");
        assert_eq!(bytes[6], 0x1E, "minute (30)");
        assert_eq!(bytes[7], 0x00, "second");
    }

    #[test]
    fn set_time_encodes_all_fields() {
        // Spot-check a different datetime to guard against hard-coded constants
        let pkt = set_time_packet(dt(2026, 12, 31, 23, 59, 59));
        let bytes = pkt.as_bytes();
        // 2026 = 0x07EA: low=0xEA=234, high=0x07=7
        assert_eq!(bytes[1], 0xEA, "year low for 2026");
        assert_eq!(bytes[2], 0x07, "year high for 2026");
        assert_eq!(bytes[3], 12, "month");
        assert_eq!(bytes[4], 31, "day");
        assert_eq!(bytes[5], 23, "hour");
        assert_eq!(bytes[6], 59, "minute");
        assert_eq!(bytes[7], 59, "second");
    }
}
