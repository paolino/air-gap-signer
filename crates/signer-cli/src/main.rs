//! Native Bluetooth client for the preprod panel.
use btleplug::{
    api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType},
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
#[derive(Serialize, Deserialize)]
struct Peer {
    device: String,
    key: [u8; 32],
}

async fn connect(known: Option<&str>) -> Result<Peripheral> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or("Bluetooth adapter unavailable")?;
    adapter
        .start_scan(ScanFilter {
            services: vec![SERVICE],
        })
        .await?;
    sleep(Duration::from_secs(4)).await;
    let mut matches = Vec::new();
    for device in adapter.peripherals().await? {
        if known.is_none_or(|id| id == device.id().to_string())
            && device
                .properties()
                .await?
                .is_some_and(|p| p.services.contains(&SERVICE))
        {
            matches.push(device);
        }
    }
    adapter.stop_scan().await?;
    if matches.len() != 1 {
        return Err(
            "expected one matching signer; check power, Bluetooth permission and nearby panels"
                .into(),
        );
    }
    let device = matches.pop().unwrap();
    if !device.is_connected().await? {
        device.connect().await?;
    }
    device.discover_services().await?;
    Ok(device)
}

#[cfg(target_os = "linux")]
async fn pair_os(device: &Peripheral) -> Result<()> {
    let session = bluer::Session::new().await?;
    // Match the adapter used by btleplug even on a multi-radio host.
    let addr = device.address().to_string().parse()?;
    for name in session.adapter_names().await? {
        let adapter = session.adapter(&name)?;
        if !adapter.device_addresses().await?.contains(&addr) {
            continue;
        }
        let peer = adapter.device(addr)?;
        if !peer.is_connected().await? {
            continue;
        }
        if !peer.is_paired().await? {
            let _agent = session
                .register_agent(bluer::agent::Agent {
                    request_confirmation: Some(Box::new(|req| {
                        Box::pin(async move {
                            eprintln!("Is {:06} on the panel? Type yes to pair.", req.passkey);
                            let yes = tokio::task::spawn_blocking(|| {
                                let mut s = String::new();
                                io::stdin().read_line(&mut s).is_ok() && s.trim() == "yes"
                            })
                            .await
                            .unwrap_or(false);
                            if yes {
                                Ok(())
                            } else {
                                Err(bluer::agent::ReqError::Rejected)
                            }
                        })
                    })),
                    ..Default::default()
                })
                .await?;
            peer.pair().await?;
        }
        return Ok(());
    }
    Err("cannot locate connected panel on a Linux adapter".into())
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
    let device = connect(Some(&peer.device)).await?;
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let dirs = directories::ProjectDirs::from("org", "air-gap-signer", "signer")
        .ok_or("no configuration directory")?;
    let path = dirs.config_dir().join("panel.json");
    match cli.command {
        Command::Pair => {
            if path.exists() {
                return Err(
                    format!("already enrolled; configuration is {}", path.display()).into(),
                );
            }
            eprintln!("On the idle panel press #5 to open pairing. Confirm the same code on both devices.");
            let device = timeout(Duration::from_secs(25), connect(None)).await??;
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            timeout(Duration::from_secs(75), pair_os(&device)).await??;
            let key=timeout(Duration::from_secs(75),public_key(&device)).await?
                .map_err(|e|format!("authenticated read failed ({e}); complete pairing in system Bluetooth settings and retry"))?;
            fs::create_dir_all(dirs.config_dir())?;
            let mut f = tempfile::NamedTempFile::new_in(dirs.config_dir())?;
            serde_json::to_writer(
                &mut f,
                &Peer {
                    device: device.id().to_string(),
                    key,
                },
            )?;
            f.flush()?;
            f.as_file().sync_all()?;
            f.persist_noclobber(path)?;
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
