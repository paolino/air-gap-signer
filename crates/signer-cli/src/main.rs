//! Native Bluetooth client for the preprod panel.
use btleplug::{
    api::{Central, Manager as _, Peripheral as _, WriteType},
    platform::{Manager, Peripheral},
};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use signer_link::{KEY, REQUEST, RESULT, SERVICE};
#[cfg(target_os = "linux")]
use std::io;
use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::{sleep, timeout, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Parser)]
#[command(about = "Pair with the panel and request a verified preprod payment witness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Confirm the matching Bluetooth code on the laptop and panel.
    Pair,
    /// Review a preprod self-payment on the paired panel.
    Sign {
        #[arg(long)]
        tx_body: PathBuf,
        /// Bench chain context; defaults to plan.json beside the transaction.
        #[arg(long)]
        plan: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Peer {
    device: String,
    key: [u8; 32],
}

// BlueZ prefixes a remote device address with the local controller index.
// That index can change when Linux reinitializes the adapter.
fn panel_id(id: &str) -> &str {
    #[cfg(target_os = "linux")]
    if let Some((index, address)) = id.strip_prefix("hci").and_then(|s| s.split_once('/')) {
        if index.parse::<u32>().is_ok() && address.starts_with("dev_") {
            return address;
        }
    }
    id
}

enum Connection<'a> {
    Pair(Option<&'a str>),
    Sign(&'a str),
}

async fn connect(request: Connection<'_>) -> Result<Peripheral> {
    let known = match request {
        Connection::Pair(id) => id,
        Connection::Sign(id) => Some(id),
    };
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or("Bluetooth adapter unavailable")?;
    #[cfg(target_os = "linux")]
    let session = bluer::Session::new().await?;
    #[cfg(target_os = "linux")]
    let _scan = {
        let address = adapter
            .adapter_address()
            .await?
            .ok_or("missing controller address")?;
        let mut radio = None;
        for name in session.adapter_names().await? {
            let candidate = session.adapter(&name)?;
            if candidate.address().await?.0 == *address.as_ref() {
                radio = Some(candidate);
                break;
            }
        }
        let radio = radio.ok_or("cannot locate the Bluetooth controller")?;
        radio.set_powered(true).await?;
        radio
            .set_discovery_filter(bluer::DiscoveryFilter {
                transport: bluer::DiscoveryTransport::Le,
                ..Default::default()
            })
            .await?;
        radio.discover_devices().await?
    };
    #[cfg(not(target_os = "linux"))]
    adapter
        .start_scan(btleplug::api::ScanFilter {
            services: vec![SERVICE],
        })
        .await?;
    sleep(Duration::from_secs(8)).await;
    let mut matches = Vec::new();
    for device in adapter.peripherals().await? {
        if known.is_none_or(|id| panel_id(id) == panel_id(&device.id().to_string()))
            && device
                .properties()
                .await?
                .is_some_and(|p| p.services.contains(&SERVICE))
        {
            matches.push(device);
        }
    }
    if matches.len() != 1 {
        #[cfg(not(target_os = "linux"))]
        let _ = adapter.stop_scan().await;
        let message = match (matches.is_empty(), known.is_some()) {
            (false, _) => "Found several signer panels. Leave only the intended panel nearby and retry.",
            (true, true) => "Bluetooth scan finished without finding your enrolled panel advertising the signer service.",
            (true, false) => "Bluetooth scan finished without finding a panel advertising the signer service.",
        };
        return Err(message.into());
    }
    let device = matches.pop().unwrap();
    // BlueZ may request authentication while opening the link. Pair through
    // the same session that owns the comparison agent before opening GATT.
    #[cfg(target_os = "linux")]
    if matches!(request, Connection::Pair(_)) {
        eprintln!("Found the signer. Checking the Bluetooth bond.");
        pair_os(&session, &device).await?;
    }
    if !device.is_connected().await? {
        device.connect().await?;
    }
    // Some BlueZ controllers end discovery when the link comes up.
    // Keep the advertisement available until connect and release our scan.
    #[cfg(not(target_os = "linux"))]
    let _ = adapter.stop_scan().await;
    device.discover_services().await?;
    Ok(device)
}

#[cfg(target_os = "linux")]
async fn pair_os(session: &bluer::Session, device: &Peripheral) -> Result<()> {
    let id = device.id().to_string();
    let (controller, _) = id.split_once('/').ok_or("unexpected BlueZ device path")?;
    let addr = device.address().to_string().parse()?;
    let peer = session.adapter(controller)?.device(addr)?;
    if peer.is_paired().await? {
        return Ok(());
    }
    let _agent = session
        .register_agent(bluer::agent::Agent {
            request_confirmation: Some(Box::new(move |req| {
                Box::pin(async move {
                    use bluer::agent::ReqError;
                    if req.device != addr {
                        return Err(ReqError::Rejected);
                    }
                    eprintln!("Is {:06} on the panel? Type yes to pair.", req.passkey);
                    use std::os::fd::AsFd;
                    let fd = io::stdin()
                        .as_fd()
                        .try_clone_to_owned()
                        .map_err(|_| ReqError::Rejected)?;
                    // SAFETY: AsyncFd owns this duplicate until deregistration.
                    let input = unsafe { tokio::io::unix::AsyncFd::register(fd) }
                        .map_err(|_| ReqError::Rejected)?;
                    let _ready = input.readable().await.map_err(|_| ReqError::Rejected)?;
                    let mut line = String::new();
                    if io::stdin().read_line(&mut line).is_ok() && line.trim() == "yes" {
                        Ok(())
                    } else {
                        Err(ReqError::Rejected)
                    }
                })
            })),
            ..Default::default()
        })
        .await?;
    if let Err(error) = peer.pair().await {
        use bluer::ErrorKind;
        if matches!(
            error.kind,
            ErrorKind::AuthenticationFailed
                | ErrorKind::AuthenticationCanceled
                | ErrorKind::AuthenticationRejected
                | ErrorKind::AuthenticationTimeout
        ) {
            return Err(format!("Found the signer, but Bluetooth pairing did not complete ({error}). Open pairing with #5 on the panel and confirm the matching code on both screens before it expires.").into());
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
async fn pair_os(device: &Peripheral) -> Result<()> {
    use windows::Devices::{
        Bluetooth::BluetoothLEDevice,
        Enumeration::{DevicePairingProtectionLevel, DevicePairingResultStatus},
    };
    let native = BluetoothLEDevice::FromBluetoothAddressAsync(device.address().into())?.await?;
    let pairing = native.DeviceInformation()?.Pairing()?;
    if !pairing.IsPaired()? {
        let status = pairing
            .PairWithProtectionLevelAsync(
                DevicePairingProtectionLevel::EncryptionAndAuthentication,
            )?
            .await?
            .Status()?;
        if status != DevicePairingResultStatus::Paired
            && status != DevicePairingResultStatus::AlreadyPaired
        {
            return Err(format!("Windows declined Bluetooth pairing ({status:?})").into());
        }
    }
    Ok(())
}

fn characteristic(p: &Peripheral, id: uuid::Uuid) -> Result<btleplug::api::Characteristic> {
    p.characteristics()
        .into_iter()
        .find(|c| c.service_uuid == SERVICE && c.uuid == id)
        .ok_or_else(|| "missing signer service characteristic".into())
}

async fn public_key(device: &Peripheral) -> Result<[u8; 32]> {
    device
        .read(&characteristic(device, KEY)?)
        .await?
        .try_into()
        .map_err(|_| "invalid payment key".into())
}

async fn sign(peer: &Peer, body: &Path, plan: Option<PathBuf>, out: &Path) -> Result<()> {
    if out.try_exists()? {
        return Err("output already exists".into());
    }
    let (payload, _) = usb_pack::cardano::prepare(fs::read(body)?)?;
    let plan = fs::read(plan.unwrap_or_else(|| body.with_file_name("plan.json")))?;
    let request = signer_link::encode(&payload, &plan)?;
    let tag = signer_link::id(&request);
    let device = connect(Connection::Sign(&peer.device)).await?;
    if public_key(&device).await? != peer.key {
        return Err("paired payment key has changed".into());
    }
    let result = characteristic(&device, RESULT)?;
    let first = device.read(&result).await?;
    if first.get(1..17) != Some(&tag) {
        if first.first() == Some(&1) {
            return Err("panel is reviewing another request".into());
        }
        let write = characteristic(&device, REQUEST)?;
        for frame in signer_link::frames(&request) {
            device
                .write(&write, &frame, WriteType::WithResponse)
                .await?;
        }
    }
    eprintln!("Review the payment on the panel. If disconnected, repeat the same command to collect the result.");
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        let reply = device.read(&result).await?;
        if reply.get(1..17) != Some(&tag) {
            return Err("the panel has no result for this request".into());
        }
        match reply[0] {
            1 if Instant::now() < deadline => sleep(Duration::from_millis(500)).await,
            1 => return Err("panel review is still pending".into()),
            2 => return Err("payment declined".into()),
            3 => return usb_pack::witness::write_verified(payload, peer.key, &reply[17..], out),
            _ => {
                return Err(
                    "panel could not complete this review; check its screen and transaction expiry"
                        .into(),
                )
            }
        }
    }
}

fn enrolled_peer(path: &Path) -> Result<Option<Peer>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn remember_peer(path: &Path, peer: &Peer) -> Result<()> {
    let check = |saved: Peer| -> Result<()> {
        if panel_id(&saved.device) == panel_id(&peer.device) && saved.key == peer.key {
            Ok(())
        } else {
            Err("panel identity or payment verification key differs from your enrollment".into())
        }
    };
    if let Some(saved) = enrolled_peer(path)? {
        return check(saved);
    }
    let dir = path.parent().ok_or("missing enrollment directory")?;
    fs::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut file, peer)?;
    file.flush()?;
    file.as_file().sync_all()?;
    match file.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            check(enrolled_peer(path)?.ok_or("enrollment changed during pairing")?)
        }
        Err(e) => Err(e.into()),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let dirs = directories::ProjectDirs::from("org", "air-gap-signer", "signer")
        .ok_or("no configuration directory")?;
    let path = dirs.config_dir().join("panel.json");
    match cli.command {
        Command::Pair => {
            let enrolled = enrolled_peer(&path)?;
            eprintln!("On the idle panel press #5 to open pairing. Confirm the same code on both devices.");
            let request = Connection::Pair(enrolled.as_ref().map(|p| p.device.as_str()));
            let device = timeout(Duration::from_secs(90), connect(request)).await??;
            #[cfg(target_os = "windows")]
            timeout(Duration::from_secs(75), pair_os(&device)).await??;
            let key=timeout(Duration::from_secs(75),public_key(&device)).await?
                .map_err(|e|format!("authenticated read failed ({e}); check that this SD carries the panel’s enrolled public key and that pairing completed"))?;
            remember_peer(
                &path,
                &Peer {
                    device: panel_id(&device.id().to_string()).into(),
                    key,
                },
            )?;
            println!("Paired. Payment verification key {}", hex::encode(key));
        }
        Command::Sign { tx_body, plan, out } => {
            let peer: Peer =
                serde_json::from_slice(&fs::read(path).map_err(|_| "run signer pair first")?)?;
            timeout(Duration::from_secs(360), sign(&peer, &tx_body, plan, &out)).await??;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn controller_renumbering_recognizes_an_existing_enrollment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("panel.json");
        let mut peer = Peer {
            device: "hci0/dev_B8_27_EB_19_1C_0A".into(),
            key: [1; 32],
        };
        remember_peer(&path, &peer).unwrap();
        let before = fs::read(&path).unwrap();
        peer.device = panel_id("hci1/dev_B8_27_EB_19_1C_0A").into();
        remember_peer(&path, &peer).unwrap();
        peer.key = [9; 32];
        assert!(remember_peer(&path, &peer).is_err());
        peer.key = [1; 32];
        peer.device = "hci1/dev_B8_27_EB_19_1C_FF".into();
        assert!(remember_peer(&path, &peer).is_err());
        assert_eq!(before, fs::read(path).unwrap());
    }

    #[test]
    fn repairing_preserves_the_original_device_and_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("panel.json");
        let mut peer = Peer {
            device: "panel".into(),
            key: [1; 32],
        };
        remember_peer(&path, &peer).unwrap();
        let before = fs::read(&path).unwrap();
        remember_peer(&path, &peer).unwrap();
        peer.key = [2; 32];
        assert!(remember_peer(&path, &peer).is_err());
        peer.key = [1; 32];
        peer.device = "another panel".into();
        assert!(remember_peer(&path, &peer).is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }
}
