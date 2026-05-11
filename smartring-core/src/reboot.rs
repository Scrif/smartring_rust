use crate::packet::Packet;

const CMD_REBOOT: u8 = 0x0C;

/// Build a reboot request packet (command 0x0C).
///
/// The ring reboots on receipt; no reply packet is sent.
pub fn reboot_request() -> Packet {
    Packet::new(CMD_REBOOT, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reboot_request_uses_command_0x0c() {
        assert_eq!(reboot_request().command, 0x0C);
    }
}
