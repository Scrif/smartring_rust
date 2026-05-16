use crate::packet::Packet;
use thiserror::Error;

pub const CMD_HR_SETTINGS: u8 = 0x16;

/// The ring's automatic heart rate logging configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct HrSettings {
    pub enabled: bool,
    /// Logging interval in minutes (1–255).
    pub interval: u8,
}

#[derive(Debug, Error, PartialEq)]
pub enum HrSettingsError {
    #[error("unexpected command byte: expected {CMD_HR_SETTINGS:#04x}, got {got:#04x}")]
    WrongCommand { got: u8 },
}

// ── Request builders ──────────────────────────────────────────────────────────

/// Build a packet to read the ring's current HR log settings.
pub fn get_hr_settings_packet() -> Packet {
    Packet::new(CMD_HR_SETTINGS, &[0x01])
}

/// Build a packet to apply `settings` to the ring.
///
/// Set request layout: `subdata[0]` = `0x02` (write marker),
/// `subdata[1]` = enabled byte (`1`=on, `2`=off), `subdata[2]` = interval.
pub fn set_hr_settings_packet(settings: &HrSettings) -> Packet {
    let enabled_byte: u8 = if settings.enabled { 1 } else { 2 };
    Packet::new(CMD_HR_SETTINGS, &[0x02, enabled_byte, settings.interval])
}

// ── Response decoder ──────────────────────────────────────────────────────────

/// Decode the ring's response to a get-settings request.
///
/// Response layout (Python reference capture
/// `b'\x16\x01\x01\x3c\x00...\x54'` = enabled, 60 min):
///
/// | `subdata[0]` | unknown — always `0x01` in observed packets |
/// | `subdata[1]` | `1` = enabled, `2` = disabled               |
/// | `subdata[2]` | interval in minutes                         |
pub fn parse_hr_settings(packet: &Packet) -> Result<HrSettings, HrSettingsError> {
    if packet.command != CMD_HR_SETTINGS {
        return Err(HrSettingsError::WrongCommand { got: packet.command });
    }
    let enabled = packet.subdata[1] == 1; // 2 = disabled; unexpected values → false
    Ok(HrSettings { enabled, interval: packet.subdata[2] })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── request packets ───────────────────────────────────────────────────────

    #[test]
    fn get_packet_uses_command_0x16() {
        assert_eq!(get_hr_settings_packet().command, CMD_HR_SETTINGS);
    }

    #[test]
    fn get_packet_subdata_is_read_marker() {
        assert_eq!(get_hr_settings_packet().subdata[0], 0x01);
    }

    #[test]
    fn set_packet_uses_command_0x16() {
        let s = HrSettings { enabled: true, interval: 30 };
        assert_eq!(set_hr_settings_packet(&s).command, CMD_HR_SETTINGS);
    }

    #[test]
    fn set_packet_encodes_write_marker() {
        let pkt = set_hr_settings_packet(&HrSettings { enabled: true, interval: 30 });
        assert_eq!(pkt.subdata[0], 0x02);
    }

    #[test]
    fn set_packet_encodes_enabled_true() {
        let pkt = set_hr_settings_packet(&HrSettings { enabled: true, interval: 30 });
        assert_eq!(pkt.subdata[1], 1);
        assert_eq!(pkt.subdata[2], 30);
    }

    #[test]
    fn set_packet_encodes_enabled_false() {
        let pkt = set_hr_settings_packet(&HrSettings { enabled: false, interval: 60 });
        assert_eq!(pkt.subdata[1], 2);
    }

    // ── parse_hr_settings — known captures from Python test suite ─────────────

    #[test]
    fn parse_enabled_60_min() {
        // Python capture: b'\x16\x01\x01\x3c\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x54'
        // command=0x16, subdata[0]=0x01, subdata[1]=0x01 (enabled), subdata[2]=0x3c=60
        let mut sub = [0u8; 14];
        sub[0] = 0x01;
        sub[1] = 0x01;
        sub[2] = 60;
        let pkt = Packet::new(CMD_HR_SETTINGS, &sub);
        assert_eq!(
            parse_hr_settings(&pkt).unwrap(),
            HrSettings { enabled: true, interval: 60 }
        );
    }

    #[test]
    fn parse_disabled_60_min() {
        // Python capture: b'\x16\x01\x02\x3c\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x55'
        // subdata[1]=0x02 → disabled
        let mut sub = [0u8; 14];
        sub[0] = 0x01;
        sub[1] = 0x02;
        sub[2] = 60;
        let pkt = Packet::new(CMD_HR_SETTINGS, &sub);
        assert_eq!(
            parse_hr_settings(&pkt).unwrap(),
            HrSettings { enabled: false, interval: 60 }
        );
    }

    #[test]
    fn parse_rejects_wrong_command() {
        let pkt = Packet::new(0x15, &[0u8; 14]);
        assert!(matches!(
            parse_hr_settings(&pkt).unwrap_err(),
            HrSettingsError::WrongCommand { got: 0x15 }
        ));
    }

    #[test]
    fn parse_unexpected_enabled_byte_defaults_to_false() {
        let mut sub = [0u8; 14];
        sub[1] = 0xFF; // not 1 or 2
        sub[2] = 30;
        let pkt = Packet::new(CMD_HR_SETTINGS, &sub);
        assert!(!parse_hr_settings(&pkt).unwrap().enabled);
    }
}
