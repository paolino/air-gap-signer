//! Talk to the FIDO2 token from the command line (each step needs a
//! touch), for acceptance against libfido2 (`scripts/fido-acceptance.sh`):
//!
//! - `fido-probe enrol` prints a new credential id (hex);
//! - `fido-probe secret <credential hex> <salt hex>` prints the token's
//!   hmac-secret for that salt (hex).

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = || -> Result<String, Box<dyn std::error::Error>> {
        let mut token = signer_pi::fido::open()?;
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["enrol"] => Ok(hex::encode(token.enrol()?)),
            ["secret", id, salt] => {
                let salt: [u8; 32] = hex::decode(salt)?
                    .try_into()
                    .map_err(|_| "salt must be 32 bytes")?;
                Ok(hex::encode(token.secret(&hex::decode(id)?, &salt)?))
            }
            _ => Err("usage: fido-probe enrol | secret <credential hex> <salt hex>".into()),
        }
    };
    match run() {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
