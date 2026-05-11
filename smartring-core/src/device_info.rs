use thiserror::Error;

use crate::packet::Packet;

/// Command bytes for device-info related requests.
const CMD_BATTERY: u8 = 0x03;
const CMD_DEVICE_INFO: u8 = 0x06;

#[derive(Debug, Error, PartialEq)]
pub enum DeviceInfoError {
    #[error("unexpected command byte: expected {expected:#04x}, got {got:#04x}")]
    WrongCommand { expected: u8, got: u8 },
}

/// Battery level reported by the ring (0–100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryLevel(pub u8);

/// Device information returned by the ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub firmware_version: String,
    pub hardware_version: String,
}

/// Build a battery-level request packet (command 0x03).
pub fn battery_request() -> Packet {
    Packet::new(CMD_BATTERY, &[])
}

/// Build a device-info request packet (command 0x06).
pub fn device_info_request() -> Packet {
    Packet::new(CMD_DEVICE_INFO, &[])
}

/// Parse a battery-level response packet.
///
/// Response layout: command=0x03, subdata[0] = battery percentage (0–100).
pub fn parse_battery(packet: &Packet) -> Result<BatteryLevel, DeviceInfoError> {
    if packet.command != CMD_BATTERY {
        return Err(DeviceInfoError::WrongCommand {
            expected: CMD_BATTERY,
            got: packet.command,
        });
    }
    Ok(BatteryLevel(packet.subdata[0]))
}

/// Parse a device-info response packet.
///
/// Response subdata layout (verify against the Python test suite with real hardware):
///   [0] fw_major  [1] fw_minor  [2] fw_patch
///   [3] hw_major  [4] hw_minor
pub fn parse_device_info(packet: &Packet) -> Result<DeviceInfo, DeviceInfoError> {
    if packet.command != CMD_DEVICE_INFO {
        return Err(DeviceInfoError::WrongCommand {
            expected: CMD_DEVICE_INFO,
            got: packet.command,
        });
    }
    let fw = format!("V{}.{}.{}", packet.subdata[0], packet.subdata[1], packet.subdata[2]);
    let hw = format!("HW{}.{}", packet.subdata[3], packet.subdata[4]);
    Ok(DeviceInfo { firmware_version: fw, hardware_version: hw })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_request_uses_command_0x03() {
        assert_eq!(battery_request().command, 0x03);
    }

    #[test]
    fn device_info_request_uses_command_0x06() {
        assert_eq!(device_info_request().command, 0x06);
    }

    #[test]
    fn parse_battery_extracts_level() {
        // Ring response: command=0x03, subdata[0]=85 → 85 %
        let pkt = Packet::new(0x03, &[85]);
        assert_eq!(parse_battery(&pkt).unwrap(), BatteryLevel(85));
    }

    #[test]
    fn parse_battery_rejects_wrong_command() {
        let pkt = Packet::new(0x06, &[85]);
        assert_eq!(
            parse_battery(&pkt).unwrap_err(),
            DeviceInfoError::WrongCommand { expected: 0x03, got: 0x06 }
        );
    }

    #[test]
    fn parse_device_info_extracts_fields() {
        // Firmware V1.7.3, hardware HW1.0
        let pkt = Packet::new(0x06, &[1, 7, 3, 1, 0]);
        let info = parse_device_info(&pkt).unwrap();
        assert_eq!(info.firmware_version, "V1.7.3");
        assert_eq!(info.hardware_version, "HW1.0");
    }

    #[test]
    fn parse_device_info_rejects_wrong_command() {
        let pkt = Packet::new(0x03, &[1, 7, 3, 1, 0]);
        assert_eq!(
            parse_device_info(&pkt).unwrap_err(),
            DeviceInfoError::WrongCommand { expected: 0x06, got: 0x03 }
        );
    }
}
