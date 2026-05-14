use thiserror::Error;

/// Wire-format error for [`Packet`].
#[derive(Debug, Error, PartialEq)]
pub enum PacketError {
    #[error("invalid checksum: expected {expected:#04x}, got {got:#04x}")]
    BadChecksum { expected: u8, got: u8 },
}

/// A 16-byte Colmi ring packet.
///
/// Layout: `[command (1)] [subdata (14)] [checksum (1)]`
///
/// The checksum is the low byte of the sum of the first 15 bytes (sum mod 256,
/// i.e. wrapping byte addition). Despite early documentation saying "mod 255",
/// the ring uses standard byte overflow — confirmed against a live device.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub command: u8,
    pub subdata: [u8; 14],
}

impl Packet {
    /// Build a packet from a command byte and up to 14 subdata bytes.
    ///
    /// Subdata shorter than 14 bytes is zero-padded; longer slices are truncated.
    pub fn new(command: u8, subdata: &[u8]) -> Self {
        let mut buf = [0u8; 14];
        let n = subdata.len().min(14);
        buf[..n].copy_from_slice(&subdata[..n]);
        Packet { command, subdata: buf }
    }

    /// Encode to wire format.
    pub fn as_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0] = self.command;
        out[1..15].copy_from_slice(&self.subdata);
        out[15] = checksum(&out[..15]);
        out
    }

    /// Parse from wire format, verifying the checksum.
    ///
    /// The ring sets bit 7 of the command byte in response packets
    /// (e.g. 0x86 for a reply to command 0x06). This bit is stripped
    /// before storing so callers can match against request command constants.
    /// Checksum validation runs on the raw bytes before any masking.
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, PacketError> {
        let expected = checksum(&bytes[..15]);
        let got = bytes[15];
        if expected != got {
            return Err(PacketError::BadChecksum { expected, got });
        }
        let mut subdata = [0u8; 14];
        subdata.copy_from_slice(&bytes[1..15]);
        Ok(Packet { command: bytes[0] & 0x7F, subdata })
    }
}

/// Low byte of the sum of all input bytes (sum mod 256).
///
/// The spec comment said "mod 255" but the ring uses standard byte-sum overflow.
/// With mod 255 a sum of e.g. 372 produces 0x75, while the ring sends 0x74
/// (372 % 256). Confirmed against a live device.
fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Battery request: command=0x03, empty subdata.
    // Expected wire bytes from the Python test suite:
    // [0x03, 0x00*14, 0x03]
    #[test]
    fn battery_request_known_bytes() {
        let pkt = Packet::new(0x03, &[]);
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[0], 0x03, "command byte");
        assert!(bytes[1..15].iter().all(|&b| b == 0), "subdata all-zero");
        assert_eq!(bytes[15], 0x03, "checksum = 0x03 mod 255");
    }

    #[test]
    fn checksum_calculation() {
        // command=0x01 with no subdata → checksum = 0x01
        let pkt = Packet::new(0x01, &[]);
        assert_eq!(pkt.as_bytes()[15], 0x01);

        // command=0x01, subdata=[0x02] → checksum = 0x03
        let pkt = Packet::new(0x01, &[0x02]);
        assert_eq!(pkt.as_bytes()[15], 0x03);
    }

    #[test]
    fn short_subdata_is_padded() {
        let pkt = Packet::new(0x05, &[0xAA, 0xBB]);
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[1], 0xAA);
        assert_eq!(bytes[2], 0xBB);
        // bytes 3–14 must be zero-padded
        assert!(bytes[3..15].iter().all(|&b| b == 0));
    }

    #[test]
    fn round_trip() {
        let original = Packet::new(0x07, &[0x01, 0x02, 0x03]);
        let bytes = original.as_bytes();
        let parsed = Packet::from_bytes(bytes).expect("valid packet");
        assert_eq!(parsed, original);
    }

    #[test]
    fn from_bytes_strips_response_bit() {
        // The ring sets bit 7 on the command byte in response packets
        // (e.g. 0x86 for a device-info reply to command 0x06).
        // from_bytes must strip this bit so parsers can match against
        // the original request command constant.
        let mut bytes = [0u8; 16];
        bytes[0] = 0x86; // device-info response
        bytes[15] = checksum(&bytes[..15]);
        let pkt = Packet::from_bytes(bytes).expect("valid response packet");
        assert_eq!(pkt.command, 0x06, "response bit must be stripped");
    }

    #[test]
    fn from_bytes_rejects_bad_checksum() {
        let mut bytes = Packet::new(0x03, &[]).as_bytes();
        bytes[15] ^= 0xFF; // corrupt checksum
        let err = Packet::from_bytes(bytes).unwrap_err();
        assert!(matches!(err, PacketError::BadChecksum { .. }));
    }

    #[test]
    fn checksum_is_mod_256_not_mod_255() {
        // sum = 255 (command=0xFF, subdata all-zero): mod 256 → 0xFF, mod 255 → 0x00
        let pkt = Packet::new(0xFF, &[]);
        assert_eq!(pkt.as_bytes()[15], 0xFF, "checksum should be 0xFF (mod 256), not 0x00 (mod 255)");

        // sum = 3825 (15 × 0xFF): 3825 as u8 = 0xF1; 3825 % 255 = 0 (wrong)
        let pkt = Packet::new(0xFF, &[0xFF; 14]);
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[15], 0xF1, "3825 mod 256 == 0xF1, not 0x00");
    }
}
