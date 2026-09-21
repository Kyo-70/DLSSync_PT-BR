#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use system_drivers::{DeviceCatalog, UpdateSource};
    let devices = system_drivers::WmiInventory.inventory()?;
    println!("Present device instances: {}", devices.len());
    println!(
        "Instances with hardware IDs: {}",
        devices
            .iter()
            .filter(|device| !device.hardware_ids.is_empty())
            .count()
    );
    println!(
        "Instances with reported problems: {}",
        devices
            .iter()
            .filter(|device| device.problem_code.is_some_and(|code| code != 0))
            .count()
    );
    if std::env::args().any(|arg| arg == "--updates") {
        let updates = system_drivers::WuaSource.search()?;
        let updates = system_drivers::filter_safe_updates(&devices, updates);
        println!("Available Windows driver updates: {}", updates.len());
        for update in updates {
            println!(
                "{} | device={:?} | installed={:?} | offered={:?} | INF={:?}",
                update.title,
                update.target_device,
                update.current_version,
                update.driver_version,
                update.target_inf
            );
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Device inventory requires Windows.");
}
