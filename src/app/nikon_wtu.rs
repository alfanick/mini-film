//! Native Nikon "Connect to PC" ingest over PTP/IP.
//!
//! Nikon Wireless Transmitter Utility ultimately receives image-transfer traffic
//! over PTP/IP. This module implements the transport and object-download side
//! directly so daemon mode can use a camera as an inbox source without depending
//! on external camera-control helpers at runtime.

use std::{
    env, fs,
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

use crate::app::util::is_supported_raw_file;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(10);
const EVENT_READ_TIMEOUT: Duration = Duration::from_millis(750);
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const TRANSFER_POLL_INTERVAL: Duration = Duration::from_millis(900);

const PTPIP_INIT_COMMAND_REQUEST: u32 = 1;
const PTPIP_INIT_COMMAND_ACK: u32 = 2;
const PTPIP_INIT_EVENT_REQUEST: u32 = 3;
const PTPIP_INIT_EVENT_ACK: u32 = 4;
const PTPIP_INIT_FAIL: u32 = 5;
const PTPIP_CMD_REQUEST: u32 = 6;
const PTPIP_CMD_RESPONSE: u32 = 7;
const PTPIP_EVENT: u32 = 8;
const PTPIP_START_DATA_PACKET: u32 = 9;
const PTPIP_DATA_PACKET: u32 = 10;
const PTPIP_END_DATA_PACKET: u32 = 12;
const PTPIP_PING: u32 = 13;
const PTPIP_PONG: u32 = 14;

const PTP_RC_OK: u16 = 0x2001;
const PTP_OC_GET_DEVICE_INFO: u16 = 0x1001;
const PTP_OC_OPEN_SESSION: u16 = 0x1002;
const PTP_OC_CLOSE_SESSION: u16 = 0x1003;
const PTP_OC_GET_OBJECT_INFO: u16 = 0x1008;
const PTP_OC_GET_PARTIAL_OBJECT: u16 = 0x101b;
const PTP_OC_NIKON_GET_NEXT_TRANSFER_OBJECT: u16 = 0x9010;
const PTP_OC_NIKON_GET_DEVICE_PTPIP_INFO: u16 = 0x90e0;
const PTP_OC_NIKON_COMPLETE_PAIRING: u16 = 0x935a;
const PTP_OC_NIKON_GET_PAIRING_CODE: u16 = 0x952b;
const PTP_EC_OBJECT_ADDED: u16 = 0x4002;
const PTP_EC_DEVICE_PROP_CHANGED: u16 = 0x4006;
const PTP_EC_CAPTURE_COMPLETE: u16 = 0x400d;

const NIKON_PAIRING_AUTHENTICATED: u32 = 0x2002;
const NIKON_PAIRING_COMPLETE: u32 = 0x2001;
const PAIRING_FINALIZE_RETRIES: usize = 5;
const NIKON_TRANSFER_ACTIVE: u8 = 2;
const NIKON_TRANSFER_OK: u32 = PTP_RC_OK as u32;
const NIKON_TRANSFER_BLOCK_SIZE: u32 = 0x80_0000;
const NIKON_TRANSFER_DRAIN_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PairingWizardState {
    NotActive,
    NeedsFinalReconnect,
}

#[derive(Clone, Debug)]
pub(crate) struct NikonWtuConfig {
    pub(crate) camera: String,
    pub(crate) port: u16,
    pub(crate) output_dir: PathBuf,
    pub(crate) computer_name: Option<String>,
    pub(crate) guid: Option<String>,
}

pub(crate) struct NikonWtuReceiver {
    stop: Arc<AtomicBool>,
    logs: Receiver<String>,
    handle: Option<JoinHandle<()>>,
}

impl NikonWtuReceiver {
    pub(crate) fn drain_logs(&self) -> Vec<String> {
        let mut logs = Vec::new();
        while let Ok(log) = self.logs.try_recv() {
            logs.push(log);
        }
        logs
    }
}

impl Drop for NikonWtuReceiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub(crate) fn start_nikon_wtu_receiver(config: NikonWtuConfig) -> Result<NikonWtuReceiver> {
    fs::create_dir_all(&config.output_dir)
        .with_context(|| format!("creating Nikon WTU inbox {}", config.output_dir.display()))?;
    let guid = resolve_guid(config.guid.as_deref())?;
    let computer_name = config
        .computer_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(default_computer_name);
    let (tx, logs) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        let mut announced_guid = false;
        while !thread_stop.load(Ordering::Relaxed) {
            if !announced_guid {
                let _ = tx.send(format!(
                    "nikon-wtu: using initiator '{}' guid {}",
                    computer_name,
                    format_guid(&guid)
                ));
                announced_guid = true;
            }
            let result = run_receiver_once(&config, &computer_name, guid, &thread_stop, &tx);
            if let Err(error) = result {
                let _ = tx.send(format!("nikon-wtu: {error:#}"));
            }
            sleep_until_stopped(&thread_stop, RECONNECT_DELAY);
        }
    });

    Ok(NikonWtuReceiver {
        stop,
        logs,
        handle: Some(handle),
    })
}

fn run_receiver_once(
    config: &NikonWtuConfig,
    computer_name: &str,
    guid: [u8; 16],
    stop: &AtomicBool,
    logs: &mpsc::Sender<String>,
) -> Result<()> {
    let mut pairing_cache = PairingCache::load().context("loading Nikon WTU pairing cache")?;
    let mut session = PtpIpSession::connect(
        &config.camera,
        config.port,
        computer_name,
        guid,
        DEFAULT_CONNECT_TIMEOUT,
    )
    .with_context(|| format!("connecting to {}:{}", config.camera, config.port))?;

    logs.send(format!(
        "nikon-wtu: connected to {} ({})",
        session.camera_name, config.camera
    ))
    .ok();

    let known_pairing = pairing_cache.is_paired(
        &config.camera,
        config.port,
        &session.camera_name,
        computer_name,
        &guid,
    );
    if known_pairing {
        logs.send(format!(
            "nikon-wtu: using cached pairing for {} ({})",
            session.camera_name,
            pairing_cache.path.display()
        ))
        .ok();
        match prepare_transfer_session(&mut session, logs) {
            Ok(()) => return transfer_loop(&mut session, &config.output_dir, stop, logs),
            Err(error) => logs
                .send(format!(
                    "nikon-wtu: cached pairing did not start transfer; retrying wizard: {error:#}"
                ))
                .ok(),
        };
        drop(session);
        thread::sleep(Duration::from_millis(500));
        session = PtpIpSession::connect(
            &config.camera,
            config.port,
            computer_name,
            guid,
            DEFAULT_CONNECT_TIMEOUT,
        )
        .with_context(|| {
            format!(
                "reconnecting to {}:{} for pairing",
                config.camera, config.port
            )
        })?;
        logs.send(format!(
            "nikon-wtu: reconnected to {} ({}) for pairing",
            session.camera_name, config.camera
        ))
        .ok();
    } else {
        logs.send(format!(
            "nikon-wtu: no cached pairing for {}; checking wizard",
            session.camera_name
        ))
        .ok();
    }

    let paired = match try_complete_pairing_wizard(&mut session, logs)? {
        PairingWizardState::NotActive => false,
        PairingWizardState::NeedsFinalReconnect => {
            drop(session);
            finalize_pairing_after_auth(config, computer_name, guid, logs)?;
            session = PtpIpSession::connect(
                &config.camera,
                config.port,
                computer_name,
                guid,
                DEFAULT_CONNECT_TIMEOUT,
            )
            .with_context(|| {
                format!(
                    "reconnecting to {}:{} after pairing",
                    config.camera, config.port
                )
            })?;
            logs.send(format!(
                "nikon-wtu: reconnected to {} ({}) after pairing",
                session.camera_name, config.camera
            ))
            .ok();
            true
        }
    };
    if paired {
        pairing_cache.upsert(
            &config.camera,
            config.port,
            &session.camera_name,
            computer_name,
            &guid,
        )?;
        logs.send(format!(
            "nikon-wtu: pairing cached in {}",
            pairing_cache.path.display()
        ))
        .ok();
    }
    logs.send("nikon-wtu: reconnecting for image transfer".to_string())
        .ok();
    drop(session);
    thread::sleep(Duration::from_millis(500));
    let mut session = PtpIpSession::connect(
        &config.camera,
        config.port,
        computer_name,
        guid,
        DEFAULT_CONNECT_TIMEOUT,
    )
    .with_context(|| format!("reconnecting to {}:{}", config.camera, config.port))?;
    logs.send(format!(
        "nikon-wtu: reconnected to {} ({})",
        session.camera_name, config.camera
    ))
    .ok();

    prepare_transfer_session(&mut session, logs)?;
    transfer_loop(&mut session, &config.output_dir, stop, logs)
}

fn prepare_transfer_session(session: &mut PtpIpSession, logs: &mpsc::Sender<String>) -> Result<()> {
    logs.send("nikon-wtu: reading PTP device info".to_string())
        .ok();
    let device_info = DeviceInfo::parse(&session.command_with_data(PTP_OC_GET_DEVICE_INFO, &[])?)?;
    logs.send(format!(
        "nikon-wtu: device info vendor=0x{:08x} version=0x{:04x} manufacturer='{}' model='{}' ops={}",
        device_info.vendor_extension_id,
        device_info.vendor_extension_version,
        device_info.manufacturer,
        device_info.model,
        format_opcodes(&device_info.operations)
    ))
    .ok();
    logs.send("nikon-wtu: opening PTP session".to_string()).ok();
    session.command_no_data(PTP_OC_OPEN_SESSION, &[1])?;
    if device_info
        .operations
        .contains(&PTP_OC_NIKON_GET_DEVICE_PTPIP_INFO)
    {
        match session.command_with_data(PTP_OC_NIKON_GET_DEVICE_PTPIP_INFO, &[]) {
            Ok(info) => logs
                .send(format!(
                    "nikon-wtu: Nikon GetDevicePTPIPInfo returned {} bytes: {}",
                    info.len(),
                    hex_preview(&info, 96)
                ))
                .ok(),
            Err(error) => logs
                .send(format!(
                    "nikon-wtu: Nikon GetDevicePTPIPInfo failed: {error:#}"
                ))
                .ok(),
        };
    } else {
        logs.send("nikon-wtu: Nikon GetDevicePTPIPInfo is not listed by this camera".to_string())
            .ok();
    }
    logs.send("nikon-wtu: ready; waiting for uploaded RAW objects".to_string())
        .ok();
    Ok(())
}

fn transfer_loop(
    session: &mut PtpIpSession,
    output_dir: &Path,
    stop: &AtomicBool,
    logs: &mpsc::Sender<String>,
) -> Result<()> {
    let mut state = TransferState::new();
    while !stop.load(Ordering::Relaxed) {
        match session.read_event(EVENT_READ_TIMEOUT)? {
            Some(event)
                if matches!(
                    event.code,
                    PTP_EC_OBJECT_ADDED | PTP_EC_DEVICE_PROP_CHANGED | PTP_EC_CAPTURE_COMPLETE
                ) =>
            {
                let force = event.code != PTP_EC_DEVICE_PROP_CHANGED;
                poll_transfer_queue(session, &mut state, output_dir, logs, force)?;
            }
            Some(event) => {
                logs.send(format!(
                    "nikon-wtu: event 0x{:04x} params {:?}",
                    event.code, event.params
                ))
                .ok();
            }
            None => poll_transfer_queue(session, &mut state, output_dir, logs, false)?,
        }
    }

    let _ = session.command_no_data(PTP_OC_CLOSE_SESSION, &[]);
    Ok(())
}

#[derive(Debug)]
struct TransferState {
    last_job_status: u32,
    last_poll: Instant,
}

impl TransferState {
    fn new() -> Self {
        Self {
            last_job_status: NIKON_TRANSFER_OK,
            last_poll: Instant::now()
                .checked_sub(TRANSFER_POLL_INTERVAL)
                .unwrap_or_else(Instant::now),
        }
    }
}

fn poll_transfer_queue(
    session: &mut PtpIpSession,
    state: &mut TransferState,
    output_dir: &Path,
    logs: &mpsc::Sender<String>,
    force: bool,
) -> Result<()> {
    if !force && state.last_poll.elapsed() < TRANSFER_POLL_INTERVAL {
        return Ok(());
    }
    state.last_poll = Instant::now();
    drain_transfer_queue(session, state, output_dir, logs)
}

fn drain_transfer_queue(
    session: &mut PtpIpSession,
    state: &mut TransferState,
    output_dir: &Path,
    logs: &mpsc::Sender<String>,
) -> Result<()> {
    for _ in 0..NIKON_TRANSFER_DRAIN_LIMIT {
        let Some(object) = query_next_transfer_object(session, state.last_job_status)? else {
            return Ok(());
        };
        if object.transfer_job != NIKON_TRANSFER_ACTIVE || object.object_handle == 0 {
            return Ok(());
        }
        match download_transfer_object(session, &object, output_dir) {
            Ok(TransferDownload::Downloaded { filename, bytes }) => {
                logs.send(format!(
                    "nikon-wtu: downloaded {filename} ({bytes} bytes, object 0x{:08x})",
                    object.object_handle
                ))
                .ok();
            }
            Ok(TransferDownload::SkippedExisting(filename)) => {
                logs.send(format!("nikon-wtu: skipped existing {filename}"))
                    .ok();
            }
            Ok(TransferDownload::SkippedUnsupported) => {}
            Err(error) if is_connection_closed_error(&error) => return Err(error),
            Err(error) => {
                logs.send(format!(
                    "nikon-wtu: failed transfer object 0x{:08x}: {error:#}",
                    object.object_handle
                ))
                .ok();
            }
        }
        state.last_job_status = NIKON_TRANSFER_OK;
    }
    Ok(())
}

fn query_next_transfer_object(
    session: &mut PtpIpSession,
    last_job_status: u32,
) -> Result<Option<TransferObject>> {
    let data = session
        .command_with_data(PTP_OC_NIKON_GET_NEXT_TRANSFER_OBJECT, &[last_job_status])
        .context("querying Nikon transfer queue")?;
    if data.is_empty() {
        return Ok(None);
    }
    let object = TransferObject::parse(&data)?;
    if object.transfer_job == NIKON_TRANSFER_ACTIVE && object.object_handle != 0 {
        Ok(Some(object))
    } else {
        Ok(None)
    }
}

fn try_complete_pairing_wizard(
    session: &mut PtpIpSession,
    logs: &mpsc::Sender<String>,
) -> Result<PairingWizardState> {
    logs.send("nikon-wtu: checking Nikon pairing wizard".to_string())
        .ok();
    if let Err(error) = session.command_no_data(PTP_OC_OPEN_SESSION, &[1]) {
        logs.send(format!(
            "nikon-wtu: pairing wizard not ready; OpenSession failed: {error:#}"
        ))
        .ok();
        return Ok(PairingWizardState::NotActive);
    }

    let pairing_code_data = match session.command_with_data(PTP_OC_NIKON_GET_PAIRING_CODE, &[]) {
        Ok(data) => data,
        Err(error) => {
            logs.send(format!(
                "nikon-wtu: no active pairing wizard detected: {error:#}"
            ))
            .ok();
            let _ = session.command_no_data(PTP_OC_CLOSE_SESSION, &[]);
            return Ok(PairingWizardState::NotActive);
        }
    };
    let pairing_code = parse_pairing_code(&pairing_code_data)?;
    logs.send(format!(
        "nikon-wtu: camera pairing code {pairing_code}; accepting wizard"
    ))
    .ok();

    match session.command_no_data(
        PTP_OC_NIKON_COMPLETE_PAIRING,
        &[NIKON_PAIRING_AUTHENTICATED],
    ) {
        Ok(_) => {
            logs.send(
                "nikon-wtu: auth step accepted; closing session before final pairing".to_string(),
            )
            .ok();
            let _ = session.command_no_data(PTP_OC_CLOSE_SESSION, &[]);
        }
        Err(error) if is_connection_closed_error(&error) => {
            logs.send(format!(
                "nikon-wtu: auth step closed the command socket; continuing with final pairing: {error:#}"
            ))
            .ok();
        }
        Err(error) => return Err(error).context("sending Nikon pairing auth step"),
    }
    Ok(PairingWizardState::NeedsFinalReconnect)
}

fn finalize_pairing_after_auth(
    config: &NikonWtuConfig,
    computer_name: &str,
    guid: [u8; 16],
    logs: &mpsc::Sender<String>,
) -> Result<()> {
    for attempt in 1..=PAIRING_FINALIZE_RETRIES {
        thread::sleep(Duration::from_millis(400));
        logs.send(format!(
            "nikon-wtu: final pairing attempt {attempt}/{PAIRING_FINALIZE_RETRIES}"
        ))
        .ok();
        let result = (|| -> Result<()> {
            let mut session = PtpIpSession::connect(
                &config.camera,
                config.port,
                computer_name,
                guid,
                DEFAULT_CONNECT_TIMEOUT,
            )
            .with_context(|| {
                format!(
                    "connecting to {}:{} for final pairing",
                    config.camera, config.port
                )
            })?;
            session.command_no_data(PTP_OC_OPEN_SESSION, &[1])?;
            match session.command_no_data(PTP_OC_NIKON_COMPLETE_PAIRING, &[NIKON_PAIRING_COMPLETE])
            {
                Ok(_) => {
                    logs.send("nikon-wtu: final pairing command accepted".to_string())
                        .ok();
                    let _ = session.command_no_data(PTP_OC_CLOSE_SESSION, &[]);
                    Ok(())
                }
                Err(error) if is_connection_closed_error(&error) => {
                    logs.send(format!(
                        "nikon-wtu: final pairing closed the command socket; treating as accepted: {error:#}"
                    ))
                    .ok();
                    Ok(())
                }
                Err(error) => Err(error).context("sending Nikon final pairing command"),
            }
        })();
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                logs.send(format!(
                    "nikon-wtu: final pairing attempt {attempt} failed: {error:#}"
                ))
                .ok();
            }
        }
    }
    bail!("Nikon final pairing did not complete after {PAIRING_FINALIZE_RETRIES} attempts")
}

fn parse_pairing_code(data: &[u8]) -> Result<String> {
    let len = read_u32(data, 0)? as usize;
    if len == 0 || len > 16 {
        bail!("invalid Nikon pairing code length {len}");
    }
    let digits = data
        .get(4..4 + len)
        .ok_or_else(|| anyhow!("short Nikon pairing code response"))?;
    let mut code = String::with_capacity(len);
    for digit in digits {
        match *digit {
            0..=9 => code.push(char::from(b'0' + *digit)),
            b'0'..=b'9' => code.push(char::from(*digit)),
            value => bail!("invalid Nikon pairing code digit 0x{value:02x}"),
        }
    }
    Ok(code)
}

struct PairingCache {
    path: PathBuf,
    records: Vec<Value>,
}

impl PairingCache {
    fn load() -> Result<Self> {
        let path = default_pairing_cache_path()?;
        let records = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| value.get("pairings").and_then(Value::as_array).cloned())
                .unwrap_or_default(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Self { path, records })
    }

    fn is_paired(
        &self,
        camera: &str,
        port: u16,
        camera_name: &str,
        computer_name: &str,
        guid: &[u8; 16],
    ) -> bool {
        let guid = format_guid(guid);
        self.records.iter().any(|record| {
            value_str(record, "guid") == Some(guid.as_str())
                && value_str(record, "computer_name") == Some(computer_name)
                && value_u64(record, "port") == Some(u64::from(port))
                && (value_str(record, "camera") == Some(camera)
                    || value_str(record, "camera_name") == Some(camera_name))
        })
    }

    fn upsert(
        &mut self,
        camera: &str,
        port: u16,
        camera_name: &str,
        computer_name: &str,
        guid: &[u8; 16],
    ) -> Result<()> {
        let guid_text = format_guid(guid);
        self.records.retain(|existing| {
            !(value_str(existing, "guid") == Some(guid_text.as_str())
                && value_str(existing, "computer_name") == Some(computer_name)
                && value_u64(existing, "port") == Some(u64::from(port))
                && (value_str(existing, "camera") == Some(camera)
                    || value_str(existing, "camera_name") == Some(camera_name)))
        });
        let record = json!({
            "camera": camera,
            "port": port,
            "camera_name": camera_name,
            "computer_name": computer_name,
            "guid": guid_text,
            "paired_at": Utc::now().to_rfc3339(),
        });
        self.records.push(record);
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(&json!({
            "version": 1,
            "pairings": &self.records,
        }))?;
        fs::write(&self.path, text).with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }
}

fn value_str<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

fn value_u64(record: &Value, key: &str) -> Option<u64> {
    record.get(key).and_then(Value::as_u64)
}

enum TransferDownload {
    Downloaded { filename: String, bytes: u64 },
    SkippedExisting(String),
    SkippedUnsupported,
}

#[derive(Debug)]
struct TransferObject {
    transfer_job: u8,
    object_handle: u32,
    object_path: String,
    object_size: u32,
}

impl TransferObject {
    fn parse(data: &[u8]) -> Result<Self> {
        let transfer_job = *data
            .first()
            .ok_or_else(|| anyhow!("empty Nikon transfer-object response"))?;
        if data.len() < 6 {
            if transfer_job == NIKON_TRANSFER_ACTIVE {
                bail!("short active Nikon transfer-object response");
            }
            return Ok(Self {
                transfer_job,
                object_handle: 0,
                object_path: String::new(),
                object_size: 0,
            });
        }

        let object_handle = read_u32(data, 1)?;
        let path_units = usize::from(data[5]);
        let (object_path, mut offset) = read_utf16_units_at(data, 6, path_units)?;
        let object_size = if offset + 4 <= data.len() {
            let size = read_u32(data, offset)?;
            offset += 4;
            size
        } else {
            0
        };

        for _ in 0..2 {
            let Some(units) = data.get(offset).copied().map(usize::from) else {
                break;
            };
            offset += 1;
            let (_, next) = read_utf16_units_at(data, offset, units)?;
            offset = next;
        }

        Ok(Self {
            transfer_job,
            object_handle,
            object_path,
            object_size,
        })
    }
}

fn download_transfer_object(
    session: &mut PtpIpSession,
    object: &TransferObject,
    output_dir: &Path,
) -> Result<TransferDownload> {
    let info_data = session
        .command_with_data(PTP_OC_GET_OBJECT_INFO, &[object.object_handle])
        .with_context(|| format!("reading object info for 0x{:08x}", object.object_handle))?;
    let info = ObjectInfo::parse(&info_data).unwrap_or_else(|_| ObjectInfo {
        filename: filename_from_transfer_path(&object.object_path)
            .unwrap_or_else(|| format!("nikon-object-{:08x}", object.object_handle)),
        size: object.object_size,
    });
    let filename = if info.filename.trim().is_empty() {
        filename_from_transfer_path(&object.object_path)
            .unwrap_or_else(|| format!("nikon-object-{:08x}", object.object_handle))
    } else {
        info.filename.clone()
    };
    if !is_supported_raw_file(Path::new(&filename)) {
        consume_transfer_object(session, object, &info)
            .with_context(|| format!("discarding unsupported transfer object {filename}"))?;
        return Ok(TransferDownload::SkippedUnsupported);
    }

    let safe_filename = sanitize_filename::sanitize(&filename);
    let output = output_dir.join(safe_filename.as_ref());
    if output.exists() {
        consume_transfer_object(session, object, &info)
            .with_context(|| format!("discarding already-downloaded transfer object {filename}"))?;
        return Ok(TransferDownload::SkippedExisting(filename));
    }
    let tmp = output.with_file_name(format!("{safe_filename}.mini-film-part"));
    let result = download_transfer_object_to_file(session, object, &info, &tmp).and_then(|bytes| {
        fs::rename(&tmp, &output)
            .with_context(|| format!("moving {} to {}", tmp.display(), output.display()))?;
        Ok(bytes)
    });

    match result {
        Ok(bytes) => Ok(TransferDownload::Downloaded { filename, bytes }),
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(error)
        }
    }
}

fn download_transfer_object_to_file(
    session: &mut PtpIpSession,
    object: &TransferObject,
    info: &ObjectInfo,
    tmp: &Path,
) -> Result<u64> {
    let expected_size = u64::from(info.size.max(object.object_size));
    let mut file = fs::File::create(tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let bytes = read_transfer_object_chunks(session, object, info, |chunk| {
        file.write_all(chunk)
            .with_context(|| format!("writing {}", tmp.display()))
    })?;

    file.flush()
        .with_context(|| format!("flushing {}", tmp.display()))?;
    drop(file);
    if expected_size > 0 && bytes != expected_size {
        // Some Nikon bodies report object size accurately before transfer.
        // Preserve the old strictness here because RAW processors tend to fail
        // later with less useful errors when a partial file slips through.
        bail!(
            "object {} expected {} bytes, received {} bytes",
            info.filename,
            expected_size,
            bytes
        );
    }
    Ok(bytes)
}

fn consume_transfer_object(
    session: &mut PtpIpSession,
    object: &TransferObject,
    info: &ObjectInfo,
) -> Result<u64> {
    read_transfer_object_chunks(session, object, info, |_| Ok(()))
}

fn read_transfer_object_chunks(
    session: &mut PtpIpSession,
    object: &TransferObject,
    info: &ObjectInfo,
    mut on_chunk: impl FnMut(&[u8]) -> Result<()>,
) -> Result<u64> {
    let expected_size = u64::from(info.size.max(object.object_size));
    let mut offset = 0u64;
    loop {
        if expected_size > 0 && offset >= expected_size {
            break;
        }
        let remaining = expected_size.saturating_sub(offset);
        let request_size = if expected_size == 0 {
            NIKON_TRANSFER_BLOCK_SIZE
        } else {
            u32::try_from(remaining.min(u64::from(NIKON_TRANSFER_BLOCK_SIZE)))
                .unwrap_or(NIKON_TRANSFER_BLOCK_SIZE)
        };
        let (chunk, _) = session.command_with_data_response(
            PTP_OC_GET_PARTIAL_OBJECT,
            &[
                object.object_handle,
                u32::try_from(offset).context("Nikon transfer object exceeds 4 GiB")?,
                request_size,
            ],
        )?;
        if chunk.is_empty() {
            if expected_size == 0 {
                break;
            }
            bail!(
                "object {} ended at {} of {} bytes",
                info.filename,
                offset,
                expected_size
            );
        }
        on_chunk(&chunk)?;
        offset = offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| anyhow!("Nikon transfer byte count overflow"))?;
        if expected_size == 0 && chunk.len() < request_size as usize {
            break;
        }
    }
    Ok(offset)
}

fn filename_from_transfer_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
}

fn sleep_until_stopped(stop: &AtomicBool, duration: Duration) {
    let mut remaining = duration;
    while !stop.load(Ordering::Relaxed) && !remaining.is_zero() {
        let step = remaining.min(Duration::from_millis(100));
        thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
}

struct PtpIpSession {
    command: TcpStream,
    event: TcpStream,
    transaction_id: u32,
    camera_name: String,
}

impl PtpIpSession {
    fn connect(
        host: &str,
        port: u16,
        computer_name: &str,
        guid: [u8; 16],
        timeout: Duration,
    ) -> Result<Self> {
        let mut command = connect_tcp(host, port, timeout)?;
        command.set_read_timeout(Some(DEFAULT_IO_TIMEOUT))?;
        command.set_write_timeout(Some(DEFAULT_IO_TIMEOUT))?;
        send_packet(
            &mut command,
            PTPIP_INIT_COMMAND_REQUEST,
            &init_command_payload(guid, computer_name),
        )?;
        let packet = read_packet(&mut command)?;
        if packet.kind == PTPIP_INIT_FAIL {
            bail!(
                "camera rejected PTP/IP init; first-time Nikon pairing/auth is probably required"
            );
        }
        if packet.kind != PTPIP_INIT_COMMAND_ACK {
            bail!("expected InitCommandAck, got packet type {}", packet.kind);
        }
        let connection_id = read_u32(&packet.payload, 0)?;
        let camera_name = utf16z_to_string(packet.payload.get(20..).unwrap_or_default());

        let mut event = connect_tcp(host, port, timeout)?;
        event.set_read_timeout(Some(EVENT_READ_TIMEOUT))?;
        event.set_write_timeout(Some(DEFAULT_IO_TIMEOUT))?;
        let mut payload = Vec::with_capacity(4);
        push_u32(&mut payload, connection_id);
        send_packet(&mut event, PTPIP_INIT_EVENT_REQUEST, &payload)?;
        let packet = read_packet(&mut event)?;
        if packet.kind != PTPIP_INIT_EVENT_ACK {
            bail!("expected InitEventAck, got packet type {}", packet.kind);
        }

        Ok(Self {
            command,
            event,
            transaction_id: 1,
            camera_name: if camera_name.is_empty() {
                "Nikon camera".to_string()
            } else {
                camera_name
            },
        })
    }

    fn command_no_data(&mut self, opcode: u16, params: &[u32]) -> Result<PtpResponse> {
        self.send_command(opcode, params)?;
        let response = self.read_response()?;
        response.ensure_ok(opcode)?;
        Ok(response)
    }

    fn command_with_data(&mut self, opcode: u16, params: &[u32]) -> Result<Vec<u8>> {
        Ok(self.command_with_data_response(opcode, params)?.0)
    }

    fn command_with_data_response(
        &mut self,
        opcode: u16,
        params: &[u32],
    ) -> Result<(Vec<u8>, PtpResponse)> {
        self.send_command(opcode, params)?;
        match self.read_data_or_response()? {
            DataRead::Data(data) => {
                let response = self.read_response()?;
                response.ensure_ok(opcode)?;
                Ok((data, response))
            }
            DataRead::Response(response) => {
                response.ensure_ok(opcode)?;
                Ok((Vec::new(), response))
            }
        }
    }

    fn send_command(&mut self, opcode: u16, params: &[u32]) -> Result<u32> {
        if params.len() > 5 {
            bail!("PTP/IP supports at most 5 command parameters");
        }
        let transaction_id = self.transaction_id;
        self.transaction_id = self.transaction_id.wrapping_add(1).max(1);
        let mut payload = Vec::with_capacity(10 + params.len() * 4);
        push_u32(&mut payload, 1);
        push_u16(&mut payload, opcode);
        push_u32(&mut payload, transaction_id);
        for param in params {
            push_u32(&mut payload, *param);
        }
        send_packet(&mut self.command, PTPIP_CMD_REQUEST, &payload)?;
        Ok(transaction_id)
    }

    fn read_data_or_response(&mut self) -> Result<DataRead> {
        let first = read_packet(&mut self.command)?;
        if first.kind == PTPIP_CMD_RESPONSE {
            return Ok(DataRead::Response(PtpResponse::parse(&first.payload)?));
        }
        if first.kind != PTPIP_START_DATA_PACKET {
            bail!("expected StartDataPacket, got packet type {}", first.kind);
        }
        let total = usize::try_from(read_u64(&first.payload, 4)?)
            .context("PTP/IP data packet length exceeds usize")?;
        let mut data = Vec::with_capacity(total);
        while data.len() < total {
            let packet = read_packet(&mut self.command)?;
            match packet.kind {
                PTPIP_DATA_PACKET | PTPIP_END_DATA_PACKET => {
                    if packet.payload.len() < 4 {
                        bail!("short data packet");
                    }
                    data.extend_from_slice(&packet.payload[4..]);
                    if packet.kind == PTPIP_END_DATA_PACKET {
                        break;
                    }
                }
                kind => bail!("unexpected packet type {kind} while reading data"),
            }
        }
        data.truncate(total);
        Ok(DataRead::Data(data))
    }

    fn read_response(&mut self) -> Result<PtpResponse> {
        loop {
            let packet = read_packet(&mut self.command)?;
            match packet.kind {
                PTPIP_CMD_RESPONSE => return PtpResponse::parse(&packet.payload),
                PTPIP_END_DATA_PACKET => continue,
                kind => bail!("expected CmdResponse, got packet type {kind}"),
            }
        }
    }

    fn read_event(&mut self, timeout: Duration) -> Result<Option<PtpEvent>> {
        self.event.set_read_timeout(Some(timeout))?;
        match read_packet(&mut self.event) {
            Ok(packet) if packet.kind == PTPIP_EVENT => Ok(Some(PtpEvent::parse(&packet.payload)?)),
            Ok(packet) if packet.kind == PTPIP_PING => {
                send_packet(&mut self.event, PTPIP_PONG, &[])?;
                Ok(None)
            }
            Ok(packet) => bail!("unexpected event-channel packet type {}", packet.kind),
            Err(error) if is_timeout_error(error.downcast_ref::<io::Error>()) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

enum DataRead {
    Data(Vec<u8>),
    Response(PtpResponse),
}

struct PtpResponse {
    code: u16,
    transaction_id: u32,
    params: Vec<u32>,
}

impl PtpResponse {
    fn parse(payload: &[u8]) -> Result<Self> {
        let code = read_u16(payload, 0)?;
        let transaction_id = read_u32(payload, 2)?;
        let mut params = Vec::new();
        let mut offset = 6;
        while offset + 4 <= payload.len() && params.len() < 5 {
            params.push(read_u32(payload, offset)?);
            offset += 4;
        }
        Ok(Self {
            code,
            transaction_id,
            params,
        })
    }

    fn ensure_ok(&self, opcode: u16) -> Result<()> {
        if self.code == PTP_RC_OK {
            return Ok(());
        }
        bail!(
            "PTP command 0x{opcode:04x} failed with response 0x{:04x}, transaction {}, params {:?}",
            self.code,
            self.transaction_id,
            self.params
        )
    }
}

struct PtpEvent {
    code: u16,
    params: Vec<u32>,
}

impl PtpEvent {
    fn parse(payload: &[u8]) -> Result<Self> {
        let code = read_u16(payload, 0)?;
        let mut params = Vec::new();
        let mut offset = 6;
        while offset + 4 <= payload.len() && params.len() < 3 {
            params.push(read_u32(payload, offset)?);
            offset += 4;
        }
        Ok(Self { code, params })
    }
}

struct ObjectInfo {
    filename: String,
    size: u32,
}

impl ObjectInfo {
    fn parse(data: &[u8]) -> Result<Self> {
        let size = read_u32(data, 8).unwrap_or(0);
        let filename = read_ptp_string(data, 52)
            .ok_or_else(|| anyhow!("object info did not include a filename"))?;
        Ok(Self { filename, size })
    }
}

struct DeviceInfo {
    vendor_extension_id: u32,
    vendor_extension_version: u16,
    operations: Vec<u16>,
    manufacturer: String,
    model: String,
}

impl DeviceInfo {
    fn parse(data: &[u8]) -> Result<Self> {
        let vendor_extension_id = read_u32(data, 2)?;
        let vendor_extension_version = read_u16(data, 6)?;
        let (_, mut offset) = read_ptp_string_at(data, 8)
            .ok_or_else(|| anyhow!("device info did not include vendor extension description"))?;
        offset = offset
            .checked_add(2)
            .ok_or_else(|| anyhow!("device info offset overflow"))?;
        let (operations, next) = read_u16_array_at(data, offset)?;
        let (_, next) = read_u16_array_at(data, next)?;
        let (_, next) = read_u16_array_at(data, next)?;
        let (_, next) = read_u16_array_at(data, next)?;
        let (_, next) = read_u16_array_at(data, next)?;
        let (manufacturer, next) = read_ptp_string_at(data, next)
            .ok_or_else(|| anyhow!("device info did not include manufacturer"))?;
        let (model, _) = read_ptp_string_at(data, next)
            .ok_or_else(|| anyhow!("device info did not include model"))?;
        Ok(Self {
            vendor_extension_id,
            vendor_extension_version,
            operations,
            manufacturer,
            model,
        })
    }
}

struct Packet {
    kind: u32,
    payload: Vec<u8>,
}

fn connect_tcp(host: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    let mut last_error = None;
    for address in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error
        .map(anyhow::Error::from)
        .unwrap_or_else(|| anyhow!("no socket addresses resolved for {host}:{port}")))
}

fn build_packet(kind: u32, payload: &[u8]) -> Result<Vec<u8>> {
    let len = 8usize
        .checked_add(payload.len())
        .ok_or_else(|| anyhow!("PTP/IP packet too large"))?;
    let len = u32::try_from(len).context("PTP/IP packet length exceeds u32")?;
    let mut buffer = Vec::with_capacity(len as usize);
    push_u32(&mut buffer, len);
    push_u32(&mut buffer, kind);
    buffer.extend_from_slice(payload);
    Ok(buffer)
}

fn send_packet(stream: &mut TcpStream, kind: u32, payload: &[u8]) -> Result<()> {
    let buffer = build_packet(kind, payload)?;
    stream.write_all(&buffer)?;
    Ok(())
}

fn read_packet(stream: &mut TcpStream) -> Result<Packet> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header)?;
    let len = read_u32(&header, 0)? as usize;
    let kind = read_u32(&header, 4)?;
    if len < 8 {
        bail!("invalid PTP/IP packet length {len}");
    }
    let mut payload = vec![0u8; len - 8];
    stream.read_exact(&mut payload)?;
    Ok(Packet { kind, payload })
}

fn init_command_payload(guid: [u8; 16], computer_name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&guid);
    push_utf16z(&mut payload, computer_name);
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 1);
    payload
}

fn push_utf16z(out: &mut Vec<u8>, value: &str) {
    for unit in value.encode_utf16() {
        push_u16(out, unit);
    }
    push_u16(out, 0);
}

fn utf16z_to_string(bytes: &[u8]) -> String {
    let mut units = Vec::new();
    for chunk in bytes.chunks_exact(2) {
        let unit = u16::from_le_bytes([chunk[0], chunk[1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

fn read_ptp_string(data: &[u8], offset: usize) -> Option<String> {
    read_ptp_string_at(data, offset).map(|(value, _)| value)
}

fn read_ptp_string_at(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let len = *data.get(offset)? as usize;
    if len == 0 {
        return Some((String::new(), offset + 1));
    }
    let start = offset + 1;
    let byte_len = len.checked_mul(2)?;
    let bytes = data.get(start..start + byte_len)?;
    Some((utf16z_to_string(bytes), start + byte_len))
}

fn read_utf16_units_at(data: &[u8], offset: usize, units: usize) -> Result<(String, usize)> {
    let byte_len = units
        .checked_mul(2)
        .ok_or_else(|| anyhow!("UTF-16 string length overflow"))?;
    let end = offset
        .checked_add(byte_len)
        .ok_or_else(|| anyhow!("UTF-16 string end offset overflow"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| anyhow!("short UTF-16 string at {offset} with {units} units"))?;
    Ok((utf16z_to_string(bytes), end))
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| anyhow!("short PTP/IP payload reading u16 at {offset}"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow!("short PTP/IP payload reading u32 at {offset}"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64> {
    let bytes = data
        .get(offset..offset + 8)
        .ok_or_else(|| anyhow!("short PTP/IP payload reading u64 at {offset}"))?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn read_u16_array_at(data: &[u8], offset: usize) -> Result<(Vec<u16>, usize)> {
    let count = read_u32(data, offset)? as usize;
    let mut next = offset
        .checked_add(4)
        .ok_or_else(|| anyhow!("PTP array offset overflow"))?;
    let byte_len = count
        .checked_mul(2)
        .ok_or_else(|| anyhow!("PTP array length overflow"))?;
    let end = next
        .checked_add(byte_len)
        .ok_or_else(|| anyhow!("PTP array end offset overflow"))?;
    data.get(next..end)
        .ok_or_else(|| anyhow!("short PTP array at {offset} with {count} entries"))?;
    let mut values = Vec::with_capacity(count);
    while next < end {
        values.push(read_u16(data, next)?);
        next += 2;
    }
    Ok((values, next))
}

fn is_timeout_error(error: Option<&io::Error>) -> bool {
    matches!(
        error.map(io::Error::kind),
        Some(io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
    )
}

fn is_connection_closed_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<io::Error>().is_some_and(|io_error| {
            matches!(
                io_error.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::BrokenPipe
            )
        })
    })
}

fn resolve_guid(explicit: Option<&str>) -> Result<[u8; 16]> {
    if let Some(value) = explicit {
        return parse_guid(value);
    }
    let path = default_guid_path()?;
    if let Ok(text) = fs::read_to_string(&path)
        && let Ok(guid) = parse_guid(text.trim())
    {
        return Ok(guid);
    }
    let guid = generated_guid();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&path, format_guid(&guid)).ok();
    Ok(guid)
}

fn default_guid_path() -> Result<PathBuf> {
    Ok(default_cache_dir()?.join("nikon-wtu-guid"))
}

fn default_pairing_cache_path() -> Result<PathBuf> {
    Ok(default_cache_dir()?.join("nikon-wtu-pairings.json"))
}

fn default_cache_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".cache").join("mini-film"))
}

fn generated_guid() -> [u8; 16] {
    let mut hasher = Sha1::new();
    hasher.update(default_computer_name().as_bytes());
    if let Some(home) = env::var_os("HOME") {
        hasher.update(home.to_string_lossy().as_bytes());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    hasher.update(now.to_le_bytes());
    let digest = hasher.finalize();
    let mut guid = [0u8; 16];
    guid.copy_from_slice(&digest[..16]);
    guid
}

fn parse_guid(value: &str) -> Result<[u8; 16]> {
    let hex = value
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .collect::<String>();
    if hex.len() != 32 {
        bail!("Nikon WTU GUID must contain exactly 16 hex bytes");
    }
    let mut guid = [0u8; 16];
    for index in 0..16 {
        guid[index] = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)?;
    }
    Ok(guid)
}

fn format_guid(guid: &[u8; 16]) -> String {
    guid.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn default_computer_name() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "mini-film".to_string())
}

fn format_opcodes(opcodes: &[u16]) -> String {
    opcodes
        .iter()
        .map(|opcode| format!("0x{opcode:04x}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn hex_preview(bytes: &[u8], max_len: usize) -> String {
    let mut out = bytes
        .iter()
        .take(max_len)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if bytes.len() > max_len {
        out.push_str(" ...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_payload_contains_guid_name_and_ptpip_version() {
        let guid = [0x11; 16];
        let payload = init_command_payload(guid, "mini");
        assert_eq!(&payload[..16], &[0x11; 16]);
        assert_eq!(&payload[16..26], b"m\0i\0n\0i\0\0\0");
        assert_eq!(&payload[26..30], &[0, 0, 1, 0]);
    }

    #[test]
    fn guid_parser_accepts_colon_and_plain_hex() {
        let expected = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        assert_eq!(
            parse_guid("00:01:02:03:04:05:06:07:08:09:0a:0b:0c:0d:0e:0f").unwrap(),
            expected
        );
        assert_eq!(
            parse_guid("000102030405060708090a0b0c0d0e0f").unwrap(),
            expected
        );
    }

    #[test]
    fn object_info_parser_reads_filename_and_size() {
        let mut data = vec![0u8; 52];
        data[8..12].copy_from_slice(&1234u32.to_le_bytes());
        let filename = "DSC_0001.NEF";
        data.push((filename.encode_utf16().count() + 1) as u8);
        for unit in filename.encode_utf16().chain(std::iter::once(0)) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        let info = ObjectInfo::parse(&data).unwrap();
        assert_eq!(info.filename, filename);
        assert_eq!(info.size, 1234);
    }

    #[test]
    fn transfer_object_parser_reads_nikon_queue_record() {
        let mut data = Vec::new();
        data.push(NIKON_TRANSFER_ACTIVE);
        push_u32(&mut data, 0x0102_0304);
        push_sized_utf16(&mut data, "NIKON/DSC_0042.NEF");
        push_u32(&mut data, 42_000_000);
        push_sized_utf16(&mut data, "20260609T120000");
        push_sized_utf16(&mut data, "20260609T120001");

        let object = TransferObject::parse(&data).unwrap();
        assert_eq!(object.transfer_job, NIKON_TRANSFER_ACTIVE);
        assert_eq!(object.object_handle, 0x0102_0304);
        assert_eq!(object.object_path, "NIKON/DSC_0042.NEF");
        assert_eq!(object.object_size, 42_000_000);
    }

    #[test]
    fn transfer_object_parser_accepts_inactive_short_record() {
        let object = TransferObject::parse(&[0]).unwrap();
        assert_eq!(object.transfer_job, 0);
        assert_eq!(object.object_handle, 0);
    }

    #[test]
    fn transfer_filename_uses_last_path_component() {
        assert_eq!(
            filename_from_transfer_path("NIKON\\Z7\\DSC_0042.NEF").as_deref(),
            Some("DSC_0042.NEF")
        );
        assert_eq!(
            filename_from_transfer_path("/store/DCIM/100NIKON/DSC_0043.NEF").as_deref(),
            Some("DSC_0043.NEF")
        );
    }

    #[test]
    fn packet_round_trip_uses_little_endian_header() {
        let bytes = build_packet(PTPIP_CMD_REQUEST, &[1, 2, 3]).unwrap();
        assert_eq!(&bytes[..8], &[11, 0, 0, 0, 6, 0, 0, 0]);
    }

    #[test]
    fn start_data_lengths_are_read_as_64_bit_values() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 7);
        payload.extend_from_slice(&0x1_0000_0001u64.to_le_bytes());
        assert_eq!(read_u64(&payload, 4).unwrap(), 0x1_0000_0001);
    }

    #[test]
    fn device_info_parser_reads_supported_operations_and_names() {
        let mut data = Vec::new();
        push_u16(&mut data, 100);
        push_u32(&mut data, 0x0000_000a);
        push_u16(&mut data, 0x0100);
        push_ptp_string(&mut data, "Nikon");
        push_u16(&mut data, 0);
        push_u16_array(
            &mut data,
            &[PTP_OC_GET_DEVICE_INFO, PTP_OC_NIKON_GET_DEVICE_PTPIP_INFO],
        );
        push_u16_array(&mut data, &[PTP_EC_OBJECT_ADDED]);
        push_u16_array(&mut data, &[]);
        push_u16_array(&mut data, &[]);
        push_u16_array(&mut data, &[]);
        push_ptp_string(&mut data, "NIKON CORPORATION");
        push_ptp_string(&mut data, "Z 7II");
        push_ptp_string(&mut data, "1.00");
        push_ptp_string(&mut data, "123");

        let info = DeviceInfo::parse(&data).unwrap();
        assert_eq!(info.vendor_extension_id, 0x0000_000a);
        assert_eq!(info.vendor_extension_version, 0x0100);
        assert_eq!(info.manufacturer, "NIKON CORPORATION");
        assert_eq!(info.model, "Z 7II");
        assert!(
            info.operations
                .contains(&PTP_OC_NIKON_GET_DEVICE_PTPIP_INFO)
        );
    }

    #[test]
    fn pairing_code_parser_reads_nikon_digit_response() {
        let mut data = Vec::new();
        push_u32(&mut data, 6);
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(parse_pairing_code(&data).unwrap(), "123456");
    }

    #[test]
    fn pairing_code_parser_accepts_ascii_digits() {
        let mut data = Vec::new();
        push_u32(&mut data, 4);
        data.extend_from_slice(b"7091");
        assert_eq!(parse_pairing_code(&data).unwrap(), "7091");
    }

    #[test]
    fn pairing_code_parser_rejects_invalid_digits() {
        let mut data = Vec::new();
        push_u32(&mut data, 3);
        data.extend_from_slice(&[1, 42, 3]);
        assert!(parse_pairing_code(&data).is_err());
    }

    #[test]
    fn pairing_socket_close_detection_matches_unexpected_eof() {
        let error = anyhow::Error::from(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "failed to fill whole buffer",
        ));
        assert!(is_connection_closed_error(&error));
        let error = anyhow::Error::from(io::Error::new(io::ErrorKind::InvalidData, "bad packet"));
        assert!(!is_connection_closed_error(&error));
    }

    #[test]
    fn pairing_cache_matches_same_identity_by_camera_name_or_host() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = PairingCache {
            path: dir.path().join("pairings.json"),
            records: Vec::new(),
        };
        let guid = [0x2a; 16];

        cache
            .upsert("192.168.0.201", 15740, "Z_7_2_6011974", "mini-film", &guid)
            .unwrap();

        assert!(cache.is_paired("192.168.0.99", 15740, "Z_7_2_6011974", "mini-film", &guid));
        assert!(cache.is_paired("192.168.0.201", 15740, "different-name", "mini-film", &guid));
        assert!(!cache.is_paired("192.168.0.99", 15740, "different-name", "mini-film", &guid));
        assert!(!cache.is_paired("192.168.0.201", 15740, "Z_7_2_6011974", "other-host", &guid));
    }

    fn push_ptp_string(out: &mut Vec<u8>, value: &str) {
        out.push((value.encode_utf16().count() + 1) as u8);
        for unit in value.encode_utf16().chain(std::iter::once(0)) {
            push_u16(out, unit);
        }
    }

    fn push_sized_utf16(out: &mut Vec<u8>, value: &str) {
        out.push((value.encode_utf16().count() + 1) as u8);
        for unit in value.encode_utf16().chain(std::iter::once(0)) {
            push_u16(out, unit);
        }
    }

    fn push_u16_array(out: &mut Vec<u8>, values: &[u16]) {
        push_u32(out, values.len() as u32);
        for value in values {
            push_u16(out, *value);
        }
    }
}
