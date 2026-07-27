use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::mpsc::{Sender, SyncSender},
    thread,
};

use anyhow::{Context, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MountedCamera {
    pub(super) device_key: String,
    pub(super) display_name: String,
    pub(super) serial: Option<String>,
    pub(super) storages: Vec<MountedStorage>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MountedStorage {
    pub(super) storage_key: String,
    pub(super) display_name: String,
    pub(super) root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MountKind {
    GPhoto2,
    Mtp,
}

impl MountKind {
    const fn scheme(self) -> &'static str {
        match self {
            Self::GPhoto2 => "gphoto2",
            Self::Mtp => "mtp",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsbIdentity {
    vendor_id: String,
    product_id: String,
    serial: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
}

pub(super) fn gvfs_root() -> Result<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .context("--auto-import requires XDG_RUNTIME_DIR from a Linux desktop session")?;
    Ok(PathBuf::from(runtime).join("gvfs"))
}

pub(super) fn discover_mounted_cameras(root: &Path) -> Result<Vec<MountedCamera>> {
    discover_mounted_cameras_with_sysfs(root, Path::new("/sys/bus/usb/devices"))
}

fn discover_mounted_cameras_with_sysfs(
    root: &Path,
    sysfs_root: &Path,
) -> Result<Vec<MountedCamera>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut cameras = BTreeMap::<String, MountedCamera>::new();
    let mut mounts = fs::read_dir(root)
        .with_context(|| format!("reading GVfs mounts from {}", root.display()))?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    mounts.sort_by_key(fs::DirEntry::file_name);

    for mount in mounts {
        let mount_name = mount.file_name().to_string_lossy().into_owned();
        let Some((kind, encoded_host)) = parse_mount_name(&mount_name) else {
            continue;
        };
        let host = percent_decode(encoded_host);
        let usb = find_usb_identity(sysfs_root, &host)?;
        let (device_key, display_name, serial) = device_identity(kind, &host, usb.as_ref());
        let storages = discover_storages(kind, &mount.path(), &mount_name)?;
        let camera = cameras
            .entry(device_key.clone())
            .or_insert_with(|| MountedCamera {
                device_key,
                display_name,
                serial,
                storages: Vec::new(),
            });
        for storage in storages {
            let mut storage = storage;
            if camera
                .storages
                .iter()
                .any(|existing| existing.storage_key == storage.storage_key)
            {
                storage.storage_key = format!("{}:{mount_name}", storage.storage_key);
            }
            camera.storages.push(storage);
        }
    }

    for camera in cameras.values_mut() {
        camera
            .storages
            .sort_by(|left, right| left.storage_key.cmp(&right.storage_key));
    }
    Ok(cameras.into_values().collect())
}

pub(super) fn start_mount_signal_listeners(reconcile: SyncSender<()>, logs: Sender<String>) {
    for interface in [
        "org.gtk.vfs.MountTracker",
        "org.gtk.Private.RemoteVolumeMonitor",
    ] {
        let reconcile = reconcile.clone();
        let logs = logs.clone();
        thread::Builder::new()
            .name(format!(
                "mini-film-auto-import-dbus-{}",
                interface.rsplit('.').next().unwrap_or("gvfs")
            ))
            .spawn(move || {
                if let Err(error) = listen_for_interface(interface, &reconcile) {
                    let _ = logs.send(format!(
                        "auto-import: GVfs D-Bus listener for {interface} stopped: {error:#}; periodic discovery remains active"
                    ));
                }
            })
            .ok();
    }
}

fn listen_for_interface(interface: &'static str, reconcile: &SyncSender<()>) -> Result<()> {
    use zbus::{
        MatchRule,
        blocking::{Connection, MessageIterator},
        message::Type,
    };

    let connection = Connection::session().context("connecting to the session D-Bus")?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(interface)?
        .build();
    let iterator = MessageIterator::for_match_rule(rule, &connection, Some(8))
        .with_context(|| format!("subscribing to {interface} signals"))?;
    for message in iterator {
        message.with_context(|| format!("receiving {interface} signal"))?;
        let _ = reconcile.try_send(());
    }
    Ok(())
}

fn parse_mount_name(name: &str) -> Option<(MountKind, &str)> {
    if let Some(host) = name.strip_prefix("gphoto2:host=") {
        return Some((MountKind::GPhoto2, host));
    }
    name.strip_prefix("mtp:host=")
        .map(|host| (MountKind::Mtp, host))
}

fn device_identity(
    kind: MountKind,
    host: &str,
    usb: Option<&UsbIdentity>,
) -> (String, String, Option<String>) {
    if let Some(usb) = usb {
        let serial = usb.serial.clone();
        let key = serial.as_ref().map_or_else(
            || format!("{}:{}", kind.scheme(), host.to_lowercase()),
            |serial| {
                format!(
                    "usb:{}:{}:{}",
                    usb.vendor_id.to_lowercase(),
                    usb.product_id.to_lowercase(),
                    serial.to_lowercase()
                )
            },
        );
        let display_name = match (&usb.manufacturer, &usb.product) {
            (Some(manufacturer), Some(product)) => {
                format!("{manufacturer} {product}")
            }
            (None, Some(product)) => product.clone(),
            _ => host.replace('_', " "),
        };
        return (key, display_name, serial);
    }
    (
        format!("{}:{}", kind.scheme(), host.to_lowercase()),
        host.replace('_', " "),
        None,
    )
}

fn discover_storages(
    kind: MountKind,
    mount_root: &Path,
    mount_name: &str,
) -> Result<Vec<MountedStorage>> {
    let mut directories = fs::read_dir(mount_root)
        .with_context(|| format!("reading camera mount {}", mount_root.display()))?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    directories.sort_by_key(fs::DirEntry::file_name);

    let direct_camera_root = directories.iter().any(|entry| {
        entry.file_name().to_str().is_some_and(|name| {
            matches!(
                name.to_ascii_uppercase().as_str(),
                "DCIM" | "MISC" | "PRIVATE"
            )
        })
    });
    let storage_directories = match kind {
        MountKind::GPhoto2 => {
            let stores = directories
                .iter()
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with("store_"))
                })
                .collect::<Vec<_>>();
            (!stores.is_empty()).then_some(stores)
        }
        MountKind::Mtp if !direct_camera_root => Some(directories.iter().collect()),
        MountKind::Mtp => None,
    };

    if let Some(storage_directories) = storage_directories {
        return Ok(storage_directories
            .into_iter()
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                MountedStorage {
                    storage_key: format!("{}:{}", kind.scheme(), name.to_lowercase()),
                    display_name: name,
                    root: entry.path(),
                }
            })
            .collect());
    }

    Ok(vec![MountedStorage {
        storage_key: format!("{}:{mount_name}", kind.scheme()),
        display_name: "camera storage".to_string(),
        root: mount_root.to_path_buf(),
    }])
}

fn find_usb_identity(sysfs_root: &Path, host: &str) -> Result<Option<UsbIdentity>> {
    if !sysfs_root.is_dir() {
        return Ok(None);
    }
    let bus_device = parse_usb_bus_device(host);
    let mut entries = fs::read_dir(sysfs_root)
        .with_context(|| format!("reading USB devices from {}", sysfs_root.display()))?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    entries.sort_by_key(fs::DirEntry::file_name);

    let mut serial_match = None;
    for entry in entries {
        let path = entry.path();
        let serial = read_optional_trimmed(&path.join("serial"))?;
        let matches_bus = bus_device.is_some_and(|(bus, device)| {
            read_number(&path.join("busnum")).ok().flatten() == Some(bus)
                && read_number(&path.join("devnum")).ok().flatten() == Some(device)
        });
        let matches_serial = serial
            .as_deref()
            .is_some_and(|serial| !serial.is_empty() && host.contains(serial));
        if !matches_bus && !matches_serial {
            continue;
        }
        let identity = UsbIdentity {
            vendor_id: read_optional_trimmed(&path.join("idVendor"))?
                .unwrap_or_else(|| "unknown".to_string()),
            product_id: read_optional_trimmed(&path.join("idProduct"))?
                .unwrap_or_else(|| "unknown".to_string()),
            serial,
            manufacturer: read_optional_trimmed(&path.join("manufacturer"))?,
            product: read_optional_trimmed(&path.join("product"))?,
        };
        if matches_bus {
            return Ok(Some(identity));
        }
        serial_match = Some(identity);
    }
    Ok(serial_match)
}

fn parse_usb_bus_device(host: &str) -> Option<(u32, u32)> {
    let marker = "usb:";
    let start = host.find(marker)? + marker.len();
    let value = &host[start..];
    let comma = value.find(',')?;
    let bus = value[..comma].trim_matches(|character: char| !character.is_ascii_digit());
    let device = value[comma + 1..]
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .unwrap_or("");
    Some((bus.parse().ok()?, device.parse().ok()?))
}

fn read_number(path: &Path) -> Result<Option<u32>> {
    Ok(read_optional_trimmed(path)?.and_then(|value| value.parse().ok()))
}

fn read_optional_trimmed(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_string()).filter(|value| !value.is_empty())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("reading USB identity {}", path.display()))
        }
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_gphoto_camera_and_each_card() {
        let temp = tempfile::tempdir().unwrap();
        let gvfs = temp.path().join("gvfs");
        let sysfs = temp.path().join("sysfs");
        let camera = gvfs.join("gphoto2:host=NIKON_NIKON_DSC_Z_7_2_0000006011974");
        fs::create_dir_all(camera.join("store_00010001")).unwrap();
        fs::create_dir_all(camera.join("store_00020001")).unwrap();
        let usb = sysfs.join("4-2");
        fs::create_dir_all(&usb).unwrap();
        fs::write(usb.join("idVendor"), "04b0\n").unwrap();
        fs::write(usb.join("idProduct"), "044b\n").unwrap();
        fs::write(usb.join("serial"), "0000006011974\n").unwrap();
        fs::write(usb.join("manufacturer"), "NIKON\n").unwrap();
        fs::write(usb.join("product"), "NIKON DSC Z 7_2\n").unwrap();

        let cameras = discover_mounted_cameras_with_sysfs(&gvfs, &sysfs).unwrap();
        assert_eq!(cameras.len(), 1);
        assert_eq!(cameras[0].device_key, "usb:04b0:044b:0000006011974");
        assert_eq!(cameras[0].storages.len(), 2);
    }

    #[test]
    fn resolves_percent_encoded_mtp_usb_address_through_sysfs() {
        let temp = tempfile::tempdir().unwrap();
        let gvfs = temp.path().join("gvfs");
        let sysfs = temp.path().join("sysfs");
        let camera = gvfs.join("mtp:host=%5Busb%3A004%2C021%5D");
        fs::create_dir_all(camera.join("SD Card")).unwrap();
        let usb = sysfs.join("4-2");
        fs::create_dir_all(&usb).unwrap();
        fs::write(usb.join("busnum"), "4\n").unwrap();
        fs::write(usb.join("devnum"), "21\n").unwrap();
        fs::write(usb.join("idVendor"), "04b0\n").unwrap();
        fs::write(usb.join("idProduct"), "044b\n").unwrap();
        fs::write(usb.join("serial"), "camera-1\n").unwrap();

        let cameras = discover_mounted_cameras_with_sysfs(&gvfs, &sysfs).unwrap();
        assert_eq!(cameras[0].device_key, "usb:04b0:044b:camera-1");
        assert_eq!(cameras[0].storages[0].display_name, "SD Card");
    }

    #[test]
    fn direct_dcim_is_treated_as_one_storage() {
        let temp = tempfile::tempdir().unwrap();
        let camera = temp.path().join("mtp:host=camera");
        fs::create_dir_all(camera.join("DCIM")).unwrap();
        fs::create_dir_all(camera.join("MISC")).unwrap();
        let storages = discover_storages(MountKind::Mtp, &camera, "mtp:host=camera").unwrap();
        assert_eq!(storages.len(), 1);
        assert_eq!(storages[0].root, camera);
    }
}
