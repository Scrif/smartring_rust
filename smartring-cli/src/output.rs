use smartring_core::steps::SportDetail;

/// Print a sport-detail slice as an aligned human-readable table.
pub fn print_steps_table(details: &[SportDetail]) {
    println!(
        "{:<25} {:>8} {:>10} {:>12}",
        "Timestamp", "Steps", "Calories", "Distance(m)"
    );
    println!("{}", "-".repeat(59));
    for d in details {
        println!(
            "{:<25} {:>8} {:>10} {:>12}",
            d.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            d.steps,
            d.calories,
            d.distance,
        );
    }
}

/// Print a sport-detail slice as RFC 4180 CSV to stdout.
///
/// All field values are numeric or ISO 8601 timestamps — no quoting required.
pub fn print_steps_csv(details: &[SportDetail]) {
    println!("timestamp,steps,calories,distance");
    for d in details {
        println!(
            "{},{},{},{}",
            d.timestamp.to_rfc3339(),
            d.steps,
            d.calories,
            d.distance,
        );
    }
}
