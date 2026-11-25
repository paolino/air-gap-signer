//! The SE051 secure element on the Pi's I2C bus, through Nitrokey's
//! `se05x` driver (T=1 over I2C and the SE05x applet APDUs).

use embedded_hal::i2c::{Error as _, ErrorKind, ErrorType, I2c, NoAcknowledgeSource, Operation};
use linux_embedded_hal::{Delay, I2cdev};
use se05x::se05x::commands::{GetRandom, GetVersion};
use se05x::se05x::Se05X;
use signer_hal::HalError;

/// The SE051's I2C address.
pub const ADDRESS: u8 = 0x48;

/// Linux I2C with errors reduced to `embedded-hal` kinds, which the
/// driver needs to tell a busy chip (NACK) from a real failure.
pub struct Bus(I2cdev);

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl I2c for Bus {
    fn transaction(&mut self, address: u8, ops: &mut [Operation<'_>]) -> Result<(), ErrorKind> {
        self.0.transaction(address, ops).map_err(|e| kind(&e))
    }
}

/// The SE051 NACKs while it is busy and the driver retries. The Pi's I2C
/// controller reports that NACK as `EREMOTEIO` on a write and as `EIO` on
/// a read (its incomplete-transfer check overrides the NACK; seen in the
/// `i2c_bcm2835` debug trace: status `DONE ERR`, failure `0x400`), which
/// the Linux HAL turns into `Other` and `Bus`. Report both as the NACK
/// they are; the driver's retry limit still ends a real fault.
fn kind(e: &linux_embedded_hal::I2CError) -> ErrorKind {
    use i2cdev::linux::LinuxI2CError;
    const EIO: i32 = 5;
    const EREMOTEIO: i32 = 121;
    let errno = match e.inner() {
        LinuxI2CError::Errno(n) => Some(*n),
        LinuxI2CError::Io(io) => io.raw_os_error(),
    };
    match errno {
        Some(EIO | EREMOTEIO) => ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address),
        _ => e.kind(),
    }
}

pub struct Se051 {
    chip: Se05X<se05x::embedded_hal::Hal10<Bus>, se05x::embedded_hal::Hal10<Delay>>,
}

fn err(what: &str, e: impl core::fmt::Debug) -> HalError {
    HalError::Storage(format!("SE051 {what}: {e:?}"))
}

impl Se051 {
    /// Open the chip on `bus` (e.g. `/dev/i2c-1`) and select its applet.
    pub fn open(bus: &str) -> Result<Self, HalError> {
        let dev = I2cdev::new(bus).map_err(|e| err("bus", e))?;
        let mut chip = Se05X::new_hal_10(Bus(dev), ADDRESS, Delay);
        chip.enable().map_err(|e| err("enable", e))?;
        Ok(Self { chip })
    }

    /// The applet version as `major.minor.patch`.
    pub fn version(&mut self) -> Result<String, HalError> {
        let buf = &mut [0u8; 64];
        let v = self
            .chip
            .run_command(&GetVersion {}, buf)
            .map_err(|e| err("version", e))?
            .version_info;
        Ok(format!("{v:?}"))
    }

    /// `n` random bytes from the chip's generator.
    pub fn random(&mut self, n: u16) -> Result<Vec<u8>, HalError> {
        let buf = &mut [0u8; 512];
        let r = self
            .chip
            .run_command(&GetRandom { length: n.into() }, buf)
            .map_err(|e| err("random", e))?;
        Ok(r.data.to_vec())
    }
}
