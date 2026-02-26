use signer_core::display::DisplayLine;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HalError {
    #[error("display error: {0}")]
    Display(String),
    #[error("button error: {0}")]
    Button(String),
    #[error("USB error: {0}")]
    Usb(String),
    #[error("storage error: {0}")]
    Storage(String),
}

/// User button action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonEvent {
    Confirm,
    Reject,
    Up,
    Down,
}

/// USB stick contents.
pub struct UsbContents {
    pub payload: Vec<u8>,
    pub interpreter_wasm: Vec<u8>,
    pub signing_spec_cbor: Vec<u8>,
}

/// Display output.
pub trait Display {
    fn clear(&mut self) -> Result<(), HalError>;
    fn show_message(&mut self, text: &str) -> Result<(), HalError>;
    fn show_lines(&mut self, lines: &[DisplayLine], scroll_offset: usize) -> Result<(), HalError>;
}

/// Button input.
pub trait Buttons {
    fn wait_event(&mut self) -> Result<ButtonEvent, HalError>;
}

/// USB mass storage mount/unmount.
pub trait UsbMount {
    fn wait_insert(&mut self) -> Result<(), HalError>;
    fn mount_readonly(&mut self) -> Result<(), HalError>;
    fn read_contents(&self) -> Result<UsbContents, HalError>;
    fn write_output(&mut self, data: &[u8]) -> Result<(), HalError>;
    fn unmount(&mut self) -> Result<(), HalError>;
}

/// Hardware secure element (ATECC608B or similar).
///
/// Private keys are generated and stored inside the chip.
/// The Pi never sees raw key material. PIN retry limits
/// are enforced in hardware.
pub trait SecureElement {
    /// Verify the user PIN. Returns remaining attempts on failure.
    fn verify_pin(&mut self, pin: &[u8]) -> Result<(), HalError>;

    /// Generate a keypair in the given slot. Returns the public key.
    fn generate_key(&mut self, slot: u8) -> Result<Vec<u8>, HalError>;

    /// Sign a hash using the key in the given slot.
    /// Requires prior PIN verification in the same session.
    fn sign(&mut self, slot: u8, hash: &[u8]) -> Result<Vec<u8>, HalError>;

    /// Read the public key from a slot.
    fn public_key(&self, slot: u8) -> Result<Vec<u8>, HalError>;
}
