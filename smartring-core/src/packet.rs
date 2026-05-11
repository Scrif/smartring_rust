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
/// The checksum is the sum of the first 15 bytes, modulo 255.
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
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, PacketError> {
        let expected = checksum(&bytes[..15]);
        let got = bytes[15];
        if expected != got {
            return Err(PacketError::BadChecksum { expected, got });
        }
        let mut subdata = [0u8; 14];
        subdata.copy_from_slice(&bytes[1..15]);
        Ok(Packet { command: bytes[0], subdata })
    }
}

/// Sum of bytes, modulo 255.
fn checksum(data: &[u8]) -> u8 {
    let sum: u32 = data.iter().map(|&b| b as u32).sum();
    (sum % 255) as u8
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
    fn from_bytes_rejects_bad_checksum() {
        let mut bytes = Packet::new(0x03, &[]).as_bytes();
        bytes[15] ^= 0xFF; // corrupt checksum
        let err = Packet::from_bytes(bytes).unwrap_err();
        assert!(matches!(err, PacketError::BadChecksum { .. }));
    }

    #[test]
    fn checksum_wraps_mod_255() {
        // 15 bytes each = 0xFF: sum = 15 * 255 = 3825; 3825 % 255 = 0
        let pkt = Packet::new(0xFF, &[0xFF; 14]);
        let bytes = pkt.as_bytes();
        assert_eq!(bytes[15], 0x00, "3825 mod 255 == 0");
    }
}
