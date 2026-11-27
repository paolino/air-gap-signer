//! Raspberry Pi implementation of the signer HAL on the prototype rig:
//! a 240x240 RGB565 framebuffer and the bonnet's GPIO controls.

#[cfg(target_os = "linux")]
pub mod bluetooth;
pub mod buttons;
pub mod display;
#[cfg(target_os = "linux")]
pub mod fido;
pub mod flow;
pub mod frame;
#[cfg(target_os = "linux")]
pub mod key_trial;
#[cfg(target_os = "linux")]
pub mod large_blob;
pub mod layout;
#[cfg(target_os = "linux")]
pub mod recovery_pin;
#[cfg(target_os = "linux")]
pub mod se051;
pub mod usb;
