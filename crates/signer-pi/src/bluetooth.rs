//! BlueZ delivers only authenticated encrypted ATT operations to these callbacks.
//! A single laptop is enrolled by numeric comparison on the physical panel.
use bluer::{
    adv::Advertisement,
    agent::{Agent, ReqError as PairError},
    gatt::local::*,
    Adapter, Address,
};
use signer_link::{Id, Mailbox, KEY, REQUEST, RESULT, SERVICE};
use std::{
    error::Error,
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

pub enum Event {
    Compare(u32, oneshot::Sender<bool>),
    Review(Vec<u8>),
}
#[derive(Default)]
struct State {
    until: Option<Instant>,
    peer: Option<Address>,
    mailbox: Mailbox,
    event: Option<Event>,
}
#[derive(Clone, Default)]
pub struct Panel(Arc<Mutex<State>>);
impl Panel {
    pub fn open_pairing(&self) {
        self.0.lock().unwrap().until = Some(Instant::now() + Duration::from_secs(60));
    }
    pub fn pairing(&self) -> bool {
        self.0
            .lock()
            .unwrap()
            .until
            .is_some_and(|end| end > Instant::now())
    }
    pub fn close_pairing(&self) {
        self.0.lock().unwrap().until = None;
    }
    pub fn ready(&self) -> bool {
        self.0.lock().unwrap().event.is_some()
    }
    pub fn take(&self) -> Option<Event> {
        self.0.lock().unwrap().event.take()
    }
    pub fn finish(&self, tag: Id, outcome: Result<Option<[u8; 64]>, ()>) {
        self.0.lock().unwrap().mailbox.finish(tag, outcome);
    }
}

async fn check_peer(panel: &Panel, adapter: &Adapter, peer: Address) -> ReqResult<()> {
    if panel.0.lock().unwrap().peer != Some(peer)
        || !adapter
            .device(peer)
            .map_err(|_| ReqError::Failed)?
            .is_paired()
            .await
            .map_err(|_| ReqError::Failed)?
    {
        return Err(ReqError::NotAuthorized);
    }
    Ok(())
}

fn read_characteristic(
    uuid: bluer::Uuid,
    panel: Panel,
    adapter: Adapter,
    key_file: PathBuf,
) -> Characteristic {
    Characteristic {
        uuid,
        read: Some(CharacteristicRead {
            read: true,
            secure_read: true,
            encrypt_authenticated_read: true,
            fun: Box::new(move |req| {
                let (panel, adapter, key_file) = (panel.clone(), adapter.clone(), key_file.clone());
                Box::pin(async move {
                    check_peer(&panel, &adapter, req.device_address).await?;
                    let bytes = if uuid == KEY {
                        let key = fs::read(key_file).map_err(|_| ReqError::Failed)?;
                        if key.len() != 32 {
                            return Err(ReqError::Failed);
                        }
                        key
                    } else {
                        panel.0.lock().unwrap().mailbox.result().to_vec()
                    };
                    bytes
                        .get(usize::from(req.offset)..)
                        .map(Vec::from)
                        .ok_or(ReqError::InvalidOffset)
                })
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub struct Radio {
    pub panel: Panel,
    _session: bluer::Session,
    _agent: bluer::agent::AgentHandle,
    _service: ApplicationHandle,
    _advertisement: bluer::adv::AdvertisementHandle,
}

pub async fn start(keys: &Path) -> Result<Radio, Box<dyn Error>> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    let path = keys
        .parent()
        .ok_or("missing signer state")?
        .join("bluetooth-peer");
    let panel = Panel::default();
    match fs::read_to_string(&path) {
        Ok(s) => panel.0.lock().unwrap().peer = Some(s.trim().parse()?),
        Err(e) if e.kind() == ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let pairing = panel.clone();
    let agent = session
        .register_agent(Agent {
            request_default: true,
            request_confirmation: Some(Box::new(move |req| {
                let panel = pairing.clone();
                let path = path.clone();
                Box::pin(async move {
                    let (tx, rx) = oneshot::channel();
                    let end = {
                        let mut s = panel.0.lock().unwrap();
                        let end = s
                            .until
                            .take()
                            .filter(|end| *end > Instant::now())
                            .ok_or(PairError::Rejected)?;
                        if s.event.is_some()
                            || s.mailbox.result().first() == Some(&1)
                            || s.peer.is_some_and(|peer| peer != req.device)
                        {
                            return Err(PairError::Rejected);
                        }
                        s.event = Some(Event::Compare(req.passkey, tx));
                        end
                    };
                    if tokio::time::timeout_at(end.into(), rx)
                        .await
                        .ok()
                        .and_then(Result::ok)
                        != Some(true)
                    {
                        return Err(PairError::Rejected);
                    }
                    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap())
                        .map_err(|_| PairError::Rejected)?;
                    writeln!(tmp, "{}", req.device).map_err(|_| PairError::Rejected)?;
                    tmp.as_file().sync_all().map_err(|_| PairError::Rejected)?;
                    tmp.persist(path).map_err(|_| PairError::Rejected)?;
                    panel.0.lock().unwrap().peer = Some(req.device);
                    Ok(())
                })
            })),
            ..Default::default()
        })
        .await?;
    adapter.set_pairable(true).await?;
    let writer = panel.clone();
    let a = adapter.clone();
    let write = Characteristic {
        uuid: REQUEST,
        write: Some(CharacteristicWrite {
            write: true,
            secure_write: true,
            encrypt_authenticated_write: true,
            method: CharacteristicWriteMethod::Fun(Box::new(move |bytes, req| {
                let (panel, adapter) = (writer.clone(), a.clone());
                Box::pin(async move {
                    check_peer(&panel, &adapter, req.device_address).await?;
                    if req.offset != 0 || req.prepare_authorize {
                        return Err(ReqError::InvalidOffset);
                    }
                    let mut s = panel.0.lock().unwrap();
                    if s.event.is_some() {
                        return Err(ReqError::InProgress);
                    }
                    if let Some(data) = s
                        .mailbox
                        .write(&bytes)
                        .map_err(|_| ReqError::NotPermitted)?
                    {
                        s.event = Some(Event::Review(data));
                    }
                    Ok(())
                })
            })),
            ..Default::default()
        }),
        ..Default::default()
    };
    let key = keys.join("signing-pin-public.key");
    let service = adapter
        .serve_gatt_application(Application {
            services: vec![Service {
                primary: true,
                uuid: SERVICE,
                characteristics: vec![
                    read_characteristic(KEY, panel.clone(), adapter.clone(), key.clone()),
                    write,
                    read_characteristic(RESULT, panel.clone(), adapter.clone(), key),
                ],
                ..Default::default()
            }],
            ..Default::default()
        })
        .await?;
    let advertisement = adapter
        .advertise(Advertisement {
            service_uuids: [SERVICE].into_iter().collect(),
            local_name: Some("Signer".into()),
            discoverable: Some(true),
            ..Default::default()
        })
        .await?;
    Ok(Radio {
        panel,
        _session: session,
        _agent: agent,
        _service: service,
        _advertisement: advertisement,
    })
}
