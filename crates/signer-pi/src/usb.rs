//! `UsbMount` on Linux: the stick is the first USB block device under
//! `/sys/block`, mounted read-only (FAT or exFAT) only for as long as it
//! takes to read the three files.

use signer_hal::{HalError, UsbContents, UsbMount};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(500);

/// Largest accepted files; anything bigger is refused before it is read
/// into memory.
pub const PAYLOAD_MAX: u64 = 1 << 20;
pub const INTERPRETER_MAX: u64 = 8 << 20;
pub const SPEC_MAX: u64 = 64 << 10;

/// The error a review ends with when the stick is pulled out mid-way.
pub const STICK_REMOVED: &str = "the USB stick was removed";

/// The only filesystems a stick may carry.
pub const FILESYSTEMS: [&str; 2] = ["vfat", "exfat"];

/// The partition to mount on the first USB disk under `sys_block`
/// (`sda1`), or the whole disk (`sda`) when it has no partitions. A disk
/// counts as USB when its sysfs path goes through a USB controller.
pub fn find_stick(sys_block: &Path) -> Option<String> {
    let mut disks: Vec<_> = fs::read_dir(sys_block).ok()?.flatten().collect();
    disks.sort_by_key(|d| d.file_name());
    disks.into_iter().find_map(|disk| {
        let target = fs::canonicalize(disk.path()).ok()?;
        if !target
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with("usb"))
        {
            return None;
        }
        let name = disk.file_name().into_string().ok()?;
        let mut partitions: Vec<String> = fs::read_dir(&target)
            .ok()?
            .flatten()
            .filter(|p| p.path().join("partition").exists())
            .filter_map(|p| p.file_name().into_string().ok())
            .collect();
        partitions.sort();
        Some(partitions.into_iter().next().unwrap_or(name))
    })
}

/// Read `path`, refusing it when larger than `max` bytes.
pub fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>, HalError> {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    let err = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => HalError::Usb(format!("the stick has no {name}")),
        _ => HalError::Usb(format!("{name}: {e}")),
    };
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(err)?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > max {
        return Err(HalError::Usb(format!("{name} is larger than {max} bytes")));
    }
    Ok(bytes)
}

/// Read the three stick files from a mounted directory.
pub fn read_stick_files(dir: &Path) -> Result<UsbContents, HalError> {
    Ok(UsbContents {
        payload: read_limited(&dir.join("payload.bin"), PAYLOAD_MAX)?,
        interpreter_wasm: read_limited(&dir.join("interpreter.wasm"), INTERPRETER_MAX)?,
        signing_spec_cbor: read_limited(&dir.join("sign.cbor"), SPEC_MAX)?,
    })
}

/// The stick on the Pi.
pub struct PiUsb {
    sys_block: PathBuf,
    mount_point: PathBuf,
    device: Option<String>,
}

impl PiUsb {
    pub fn new(sys_block: PathBuf, mount_point: PathBuf) -> Self {
        Self {
            sys_block,
            mount_point,
            device: None,
        }
    }

    /// Remember the device found by this scan so opening the Keys menu stays
    /// possible if a stick is removed between polls.
    pub fn poll_insert(&mut self) -> bool {
        self.device = find_stick(&self.sys_block);
        self.device.is_some()
    }

    /// Block until the stick has been pulled out.
    pub fn wait_removed(&mut self) {
        while find_stick(&self.sys_block).is_some() {
            thread::sleep(POLL);
        }
        self.device = None;
    }

    fn run(&self, cmd: &mut Command) -> Result<(), HalError> {
        let out = cmd
            .output()
            .map_err(|e| HalError::Usb(format!("{cmd:?}: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(HalError::Usb(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }
}

impl UsbMount for PiUsb {
    fn wait_insert(&mut self) -> Result<(), HalError> {
        loop {
            if self.poll_insert() {
                return Ok(());
            }
            thread::sleep(POLL);
        }
    }

    /// FAT or exFAT only (large sticks come formatted exFAT), read-only,
    /// and nothing on it can be executed or opened as a device.
    fn mount_readonly(&mut self) -> Result<(), HalError> {
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| HalError::Usb("no stick inserted".into()))?;
        fs::create_dir_all(&self.mount_point)
            .map_err(|e| HalError::Usb(format!("{}: {e}", self.mount_point.display())))?;
        for fs_type in FILESYSTEMS {
            let mounted = self.run(
                Command::new("mount")
                    .args(["-t", fs_type, "-o", "ro,nosuid,nodev,noexec"])
                    .arg(Path::new("/dev").join(device))
                    .arg(&self.mount_point),
            );
            if mounted.is_ok() {
                return Ok(());
            }
        }
        Err(HalError::Usb(format!(
            "/dev/{device} is not a FAT or exFAT stick"
        )))
    }

    fn read_contents(&self) -> Result<UsbContents, HalError> {
        read_stick_files(&self.mount_point)
    }

    fn write_output(&mut self, _: &[u8]) -> Result<(), HalError> {
        Err(HalError::Usb(
            "writing to the stick is not supported yet".into(),
        ))
    }

    fn read_file(&self, name: &str) -> Result<Option<Vec<u8>>, HalError> {
        let path = self.mount_point.join(name);
        if path.exists() {
            read_limited(&path, PAYLOAD_MAX).map(Some)
        } else {
            Ok(None)
        }
    }

    fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), HalError> {
        Err(HalError::Usb(
            "writing to the stick is not supported yet".into(),
        ))
    }

    fn unmount(&mut self) -> Result<(), HalError> {
        self.run(Command::new("umount").arg(&self.mount_point))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    /// A fake `/sys/block`: each disk is a symlink to a device directory,
    /// as the kernel lays it out.
    fn sys_block(disks: &[(&str, &str, &[&str])]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let block = root.path().join("block");
        fs::create_dir(&block).unwrap();
        for (name, bus_path, partitions) in disks {
            let dev = root.path().join("devices").join(bus_path).join(name);
            fs::create_dir_all(&dev).unwrap();
            for p in *partitions {
                fs::create_dir(dev.join(p)).unwrap();
                fs::write(dev.join(p).join("partition"), "1\n").unwrap();
            }
            symlink(&dev, block.join(name)).unwrap();
        }
        root
    }

    #[test]
    fn finds_the_first_partition_of_a_usb_disk() {
        let root = sys_block(&[
            ("mmcblk0", "platform/mmc", &["mmcblk0p1"]),
            ("sda", "platform/usb1/1-1/host0", &["sda1", "sda2"]),
        ]);
        assert_eq!(find_stick(&root.path().join("block")), Some("sda1".into()));
    }

    #[test]
    fn uses_the_whole_disk_when_it_has_no_partitions() {
        let root = sys_block(&[("sdb", "platform/usb1/1-2/host1", &[])]);
        assert_eq!(find_stick(&root.path().join("block")), Some("sdb".into()));
    }

    #[test]
    fn ignores_disks_that_are_not_on_usb() {
        let root = sys_block(&[
            ("mmcblk0", "platform/mmc", &["mmcblk0p1"]),
            ("loop0", "virtual", &[]),
        ]);
        assert_eq!(find_stick(&root.path().join("block")), None);
    }

    #[test]
    fn refuses_a_file_over_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("payload.bin");
        fs::write(&f, vec![0u8; 11]).unwrap();
        assert_eq!(read_limited(&f, 11).unwrap().len(), 11);
        assert!(read_limited(&f, 10).is_err());
    }

    #[test]
    fn reads_the_three_files_and_names_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("payload.bin"), b"p").unwrap();
        fs::write(dir.path().join("interpreter.wasm"), b"w").unwrap();
        let err = read_stick_files(dir.path())
            .err()
            .expect("sign.cbor is missing");
        assert!(
            err.to_string().contains("the stick has no sign.cbor"),
            "{err}"
        );
        fs::write(dir.path().join("sign.cbor"), b"s").unwrap();
        let c = read_stick_files(dir.path()).unwrap();
        assert_eq!(
            (c.payload, c.interpreter_wasm, c.signing_spec_cbor),
            (b"p".to_vec(), b"w".to_vec(), b"s".to_vec())
        );
    }
}
