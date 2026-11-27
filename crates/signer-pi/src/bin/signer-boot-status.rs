//! Started early in both boot stages. Release the display between frames so
//! the interactive panel can take permanent ownership when it is ready.
#[cfg(target_os = "linux")]
fn main() {
    use signer_hal::Display;
    use signer_pi::{
        boot,
        display::{find_framebuffer, FbDisplay},
    };
    use std::{
        fs,
        path::Path,
        time::{Duration, Instant},
    };
    let sys = Path::new("/sys/class/graphics");
    let mut previous = String::new();
    let mut next = Instant::now();
    loop {
        let mut phase = fs::read_to_string(boot::PHASE)
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|_| "devices".into());
        if phase == "ready" {
            return;
        }
        let first = Path::new("/run/signer-first-boot").exists()
            || Path::new("/mnt-root/nix-path-registration").exists()
            || Path::new("/nix-path-registration").exists();
        if phase == "first" {
            for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
                if entry
                    .file_name()
                    .as_encoded_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_digit)
                {
                    match fs::read_to_string(entry.path().join("comm")).as_deref() {
                        Ok("resize2fs\n" | "sfdisk\n" | "partprobe\n") => {
                            phase = "resize".into();
                            break;
                        }
                        Ok("nix-store\n") => {
                            phase = "register".into();
                            break;
                        }
                        _ => (),
                    }
                }
            }
        }
        let now = Instant::now();
        if phase != previous || now >= next {
            let text = boot::message(&phase, boot::uptime(), boot::estimate(first));
            // The kernel log makes the same stages observable over a serial console.
            let _ = fs::write(
                "/dev/kmsg",
                format!(
                    "<6>signer-boot: {}\n",
                    text.lines()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            );
            if let Some(fb) = find_framebuffer(sys, "fb_st7789v") {
                if let Ok(mut display) = FbDisplay::open(sys, &fb, 18.0) {
                    let mut lines = text.lines();
                    let view = signer_core::review::View::Detail {
                        header: lines.next().unwrap_or_default().to_owned(),
                        rows: lines.map(str::to_owned).collect(),
                    };
                    let _ = display.show_review(&view);
                }
            }
            previous = phase;
            next = now + Duration::from_secs(5);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Boot feedback uses the Pi framebuffer");
}
