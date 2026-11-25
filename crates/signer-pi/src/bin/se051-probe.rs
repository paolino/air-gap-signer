//! Check that the SE051 answers: select its applet, read its version and
//! a few random bytes.

#[cfg(target_os = "linux")]
fn main() {
    use signer_pi::se051::Se051;
    let bus = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/dev/i2c-1".into());
    let run = || -> Result<(), signer_hal::HalError> {
        let mut se = Se051::open(&bus)?;
        println!("version: {}", se.version()?);
        println!("random:  {}", hex::encode(se.random(16)?));
        Ok(())
    };
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("se051-probe needs Linux I2C");
    std::process::exit(1);
}
