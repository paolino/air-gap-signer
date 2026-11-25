//! Talk to the FIDO2 token from the command line (each step needs a
//! touch), for acceptance against libfido2 (`scripts/fido-acceptance.sh`).
//! The token's PIN comes from the `FIDO_PIN` environment variable.
//!
//! - `fido-probe enrol` prints a new credential id (hex);
//! - `fido-probe secret <credential hex> <salt hex>` prints the token's
//!   hmac-secret for that salt (hex);
//! - `fido-probe blob-put <credential hex> <file>` stores the file in the
//!   token under that credential;
//! - `fido-probe blob-get` prints the resident credential and the data
//!   stored under it (hex).

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = || -> Result<String, Box<dyn std::error::Error>> {
        let pin = std::env::var("FIDO_PIN").map_err(|_| "set FIDO_PIN")?;
        let mut token = signer_pi::fido::open()?;
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["enrol"] => Ok(hex::encode(token.enrol(&pin)?)),
            ["retries"] => Ok(token.pin_retries()?.to_string()),
            ["blob-put", id, file] => {
                token.store_backup(&hex::decode(id)?, &std::fs::read(file)?, &pin)?;
                Ok("stored".into())
            }
            ["blob-get"] => {
                let (id, data) = token.load_backup(&pin)?;
                Ok(format!("{} {}", hex::encode(id), hex::encode(data)))
            }
            ["secret", id, salt] => {
                let salt: [u8; 32] = hex::decode(salt)?
                    .try_into()
                    .map_err(|_| "salt must be 32 bytes")?;
                Ok(hex::encode(token.secret(&hex::decode(id)?, &salt, &pin)?))
            }
            _ => {
                Err("usage: fido-probe enrol | retries | secret <credential hex> <salt hex> | blob-put <credential hex> <file> | blob-get".into())
            }
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
