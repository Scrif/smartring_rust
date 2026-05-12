use anyhow::Result;
use clap::Args;
use smartring_core::client::Client;
use smartring_core::packet::Packet;

#[derive(Debug, Args)]
pub struct RawArgs {
    /// Command byte to send (decimal or 0x-prefixed hex, e.g. "3" or "0x03")
    #[arg(long, value_parser = parse_hex_u8)]
    pub command: u8,

    /// Optional subdata bytes as a hex string (e.g. "0102AABB")
    #[arg(long, value_parser = parse_hex_bytes, default_value = "")]
    pub subdata: Vec<u8>,

    /// Number of reply packets to collect (0 = fire and forget)
    #[arg(long, default_value_t = 1)]
    pub replies: usize,
}

pub async fn run(args: &RawArgs, client: &Client) -> Result<()> {
    let packet = Packet::new(args.command, &args.subdata);
    let replies = client.send_recv(packet, args.replies).await?;

    if replies.is_empty() {
        println!("(no reply)");
    } else {
        for reply in &replies {
            let hex: Vec<String> = reply.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
            println!("{}", hex.join(" "));
        }
    }

    Ok(())
}

/// Accept "3", "03", "0x03", "0X03" → u8.
fn parse_hex_u8(s: &str) -> Result<u8, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u8>().map_err(|e| e.to_string())
    }
}

/// Accept a hex string (with or without 0x prefix) → Vec<u8>.
fn parse_hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(vec![]);
    }
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.len() % 2 != 0 {
        return Err(format!("hex string must have an even number of digits, got {}", s.len()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_u8_decimal() {
        assert_eq!(parse_hex_u8("3").unwrap(), 3);
        assert_eq!(parse_hex_u8("255").unwrap(), 255);
    }

    #[test]
    fn parse_hex_u8_hex_prefix() {
        assert_eq!(parse_hex_u8("0x03").unwrap(), 3);
        assert_eq!(parse_hex_u8("0xFF").unwrap(), 255);
        assert_eq!(parse_hex_u8("0X1A").unwrap(), 26);
    }

    #[test]
    fn parse_hex_bytes_empty() {
        assert_eq!(parse_hex_bytes("").unwrap(), vec![]);
    }

    #[test]
    fn parse_hex_bytes_valid() {
        assert_eq!(parse_hex_bytes("0102AABB").unwrap(), vec![0x01, 0x02, 0xAA, 0xBB]);
        assert_eq!(parse_hex_bytes("0x0102").unwrap(), vec![0x01, 0x02]);
    }

    #[test]
    fn parse_hex_bytes_odd_length_errors() {
        assert!(parse_hex_bytes("ABC").is_err());
    }
}
