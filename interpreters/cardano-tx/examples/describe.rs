//! Print the review JSON for a transaction CBOR file:
//! `cargo run -p cardano-tx --example describe -- tx.cbor`

fn main() {
    let path = std::env::args().nth(1).expect("usage: describe <tx.cbor>");
    let cbor = std::fs::read(&path).expect("readable file");
    match cardano_tx::describe(&cbor) {
        Ok(json) => println!("{json:#}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
