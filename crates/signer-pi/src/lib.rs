//! Raspberry Pi implementation of the signer HAL on the prototype rig:
//! a 240x240 RGB565 framebuffer and the bonnet's GPIO controls.

pub mod buttons;
pub mod display;
pub mod flow;
pub mod frame;
pub mod layout;
pub mod usb;
