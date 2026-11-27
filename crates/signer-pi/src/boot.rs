//! Boot timing uses kernel uptime so the initrd and the main system share a clock.
use std::{fs, path::Path};
pub const PHASE: &str = "/run/signer-boot-phase";
const LAST: &str = "/var/lib/signer-appliance/last-normal-boot-seconds";

pub fn uptime() -> u64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split('.').next()?.parse().ok())
        .unwrap_or(0)
}
pub fn phase(value: &str) {
    let _ = fs::write(PHASE, value);
}
pub fn ready() {
    phase("ready");
    if !Path::new("/run/signer-first-boot").exists()
        && fs::File::create_new("/run/signer-boot-finished").is_ok()
    {
        let _ = fs::write(LAST, uptime().to_string());
    }
}
pub fn estimate(first: bool) -> u64 {
    if first {
        return 180;
    }
    fs::read_to_string(LAST)
        .ok()
        .or_else(|| fs::read_to_string(format!("/mnt-root{LAST}")).ok())
        .and_then(|s| s.trim().parse().ok())
        .filter(|n| (5..=3600).contains(n))
        .unwrap_or(60)
}
pub fn message(stage: &str, elapsed: u64, budget: u64) -> String {
    let (title, details) = match stage {
        "devices" => (
            "Finding devices",
            ["Loading board drivers", "Looking for storage"],
        ),
        "mount" => (
            "Opening the SD card",
            ["Checking filesystem", "Waiting for root disk"],
        ),
        "activation" => (
            "Starting NixOS",
            ["Loading system setup", "Applying boot config"],
        ),
        "resize" => (
            "Preparing SD card",
            ["First boot setup", "Expanding storage"],
        ),
        "register" => (
            "Indexing software",
            ["First boot setup", "Registering packages"],
        ),
        "first" => (
            "Preparing the system",
            ["First boot setup", "Keep power connected"],
        ),
        "radio" => (
            "Starting Bluetooth",
            ["Opening the radio", "Waiting for Bluetooth"],
        ),
        "panel" => (
            "Opening controls",
            ["Preparing the display", "Starting panel input"],
        ),
        "failed" => ("Startup stopped", ["SD setup failed", "Check boot journal"]),
        _ => (
            "Starting services",
            ["Loading the signer", "Opening services"],
        ),
    };
    let eta = if stage == "failed" {
        "ETA unavailable".into()
    } else if elapsed >= budget {
        format!("ETA overdue +{}s", elapsed - budget)
    } else {
        format!("ETA ~{}s", budget - elapsed)
    };
    format!(
        "SIGNER STARTUP\n\n{title}\n{}\n\nElapsed {elapsed}s\n{eta}",
        details[(elapsed / 5 % 2) as usize]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_five_seconds_changes_the_feedback_even_after_estimate_expires() {
        for stage in [
            "devices",
            "mount",
            "activation",
            "resize",
            "register",
            "first",
            "radio",
            "panel",
            "failed",
            "services",
        ] {
            for t in (0..200).step_by(5) {
                let msg = message(stage, t, 60);
                assert_ne!(msg, message(stage, t + 5, 60));
                assert!(msg.contains("ETA"));
                assert!(msg.lines().all(|l| l.len() <= 22));
            }
        }
        assert!(message("radio", 75, 60).contains("overdue +15s"));
    }
}
