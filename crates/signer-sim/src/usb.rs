use signer_hal::{HalError, UsbContents, UsbMount};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Directory-based USB simulation.
///
/// Watches a directory for `payload.bin`, `interpreter.wasm`, and `sign.cbor`.
/// Writes output as `signed.bin`.
pub struct SimUsb {
    dir: PathBuf,
}

impl SimUsb {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn payload_path(&self) -> PathBuf {
        self.dir.join("payload.bin")
    }

    fn interpreter_path(&self) -> PathBuf {
        self.dir.join("interpreter.wasm")
    }

    fn spec_path(&self) -> PathBuf {
        self.dir.join("sign.cbor")
    }

    fn output_path(&self) -> PathBuf {
        self.dir.join("signed.bin")
    }

    fn files_present(&self) -> bool {
        self.payload_path().exists()
            && self.interpreter_path().exists()
            && self.spec_path().exists()
    }
}

impl UsbMount for SimUsb {
    fn wait_insert(&mut self) -> Result<(), HalError> {
        while !self.files_present() {
            thread::sleep(POLL_INTERVAL);
        }
        Ok(())
    }

    fn mount_readonly(&mut self) -> Result<(), HalError> {
        // no-op for directory simulation
        Ok(())
    }

    fn read_contents(&self) -> Result<UsbContents, HalError> {
        let payload = fs::read(self.payload_path()).map_err(|e| HalError::Usb(e.to_string()))?;
        let interpreter_wasm =
            fs::read(self.interpreter_path()).map_err(|e| HalError::Usb(e.to_string()))?;
        let signing_spec_cbor =
            fs::read(self.spec_path()).map_err(|e| HalError::Usb(e.to_string()))?;
        Ok(UsbContents {
            payload,
            interpreter_wasm,
            signing_spec_cbor,
        })
    }

    fn write_output(&mut self, data: &[u8]) -> Result<(), HalError> {
        fs::write(self.output_path(), data).map_err(|e| HalError::Usb(e.to_string()))
    }

    fn read_file(&self, name: &str) -> Result<Option<Vec<u8>>, HalError> {
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        fs::read(&path)
            .map(Some)
            .map_err(|e| HalError::Usb(e.to_string()))
    }

    fn write_file(&mut self, name: &str, data: &[u8]) -> Result<(), HalError> {
        fs::write(self.dir.join(name), data).map_err(|e| HalError::Usb(e.to_string()))
    }

    fn unmount(&mut self) -> Result<(), HalError> {
        // no-op for directory simulation
        Ok(())
    }
}
