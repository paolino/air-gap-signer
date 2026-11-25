//! Raspberry Pi implementation of the signer HAL on the prototype rig:
//! a 240x240 RGB565 framebuffer and the bonnet's GPIO controls.

pub mod buttons;
pub mod display;
pub mod flow;
pub mod frame;
pub mod layout;
#[cfg(target_os = "linux")]
pub mod se051;
pub mod usb;
