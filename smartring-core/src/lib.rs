/// smartring-core: protocol logic and BLE client for Colmi-family smart rings.
///
/// Public modules are added as each feature is implemented. See `tasks/plan.md`
/// for the implementation order.

pub mod ble;
pub mod client;
pub mod device_info;
pub mod hr;
pub mod packet;
pub mod reboot;
pub mod set_time;

#[cfg(test)]
mod tests {
    /// Smoke test — confirms the test harness itself is wired up correctly.
    #[test]
    fn smoke() {
        assert_eq!(2 + 2, 4);
    }
}
