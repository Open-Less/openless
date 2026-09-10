use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use uuid::Uuid;

const CACHE_FILE: &str = "remote-input-ips-v1.txt";
const MAX_CACHE_BYTES: u64 = 1024;
const MAX_CACHE_IPS: usize = 32;
const ENUMERATION_TIMEOUT: Duration = Duration::from_secs(1);
const ROUTE_DESTINATIONS: [&str; 4] = [
    "8.8.8.8:80",
    "1.1.1.1:80",
    "192.168.8.1:80",
    "192.168.1.1:80",
];

static ENUMERATION_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanAddressSnapshot {
    pub(crate) ips: Vec<Ipv4Addr>,
    pub(crate) stale: bool,
}

pub(crate) fn normalize_lan_ips<I>(ips: I) -> Vec<Ipv4Addr>
where
    I: IntoIterator<Item = Ipv4Addr>,
{
    let mut normalized = ips.into_iter().filter(is_private_lan).collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized.dedup();
    normalized.truncate(MAX_CACHE_IPS);
    normalized
}

pub(crate) fn discover_lan_addresses(config_dir: Option<&Path>) -> LanAddressSnapshot {
    let live_ips = enumerate_interfaces_with_timeout();
    if let Some(ips) = live_ips.as_ref().filter(|ips| !ips.is_empty()) {
        if let Some(dir) = config_dir {
            if let Err(error) = persist_cached_ips(dir, ips) {
                log::warn!("[remote-input] persist LAN IP cache failed: {error}");
            }
        }
    }
    let cached_ips = if live_ips.as_ref().is_some_and(|ips| !ips.is_empty()) {
        None
    } else {
        config_dir.and_then(load_cached_ips)
    };
    let route_ips = if live_ips.as_ref().is_some_and(|ips| !ips.is_empty())
        || cached_ips.as_ref().is_some_and(|ips| !ips.is_empty())
    {
        Vec::new()
    } else {
        local_lan_ipv4s_from_route()
    };
    let snapshot = choose_snapshot(live_ips, cached_ips, route_ips);
    if snapshot.stale {
        log::info!("[remote-input] using cached LAN IPs after interface discovery timeout");
    }
    snapshot
}

fn choose_snapshot(
    live_ips: Option<Vec<Ipv4Addr>>,
    cached_ips: Option<Vec<Ipv4Addr>>,
    route_ips: Vec<Ipv4Addr>,
) -> LanAddressSnapshot {
    if let Some(ips) = live_ips.filter(|ips| !ips.is_empty()) {
        return LanAddressSnapshot { ips, stale: false };
    }
    if let Some(ips) = cached_ips.filter(|ips| !ips.is_empty()) {
        return LanAddressSnapshot { ips, stale: true };
    }
    LanAddressSnapshot {
        ips: normalize_lan_ips(route_ips),
        stale: false,
    }
}

fn is_private_lan(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    !ip.is_loopback()
        && !ip.is_link_local()
        && ((octets[0] == 192 && octets[1] == 168)
            || octets[0] == 10
            || (octets[0] == 172 && (16..=31).contains(&octets[1])))
}

fn enumerate_interfaces_with_timeout() -> Option<Vec<Ipv4Addr>> {
    if ENUMERATION_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return None;
    }

    let (tx, rx) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("openless-lan-addresses".to_string())
        .spawn(move || {
            struct ResetInFlight;

            impl Drop for ResetInFlight {
                fn drop(&mut self) {
                    ENUMERATION_IN_FLIGHT.store(false, Ordering::Release);
                }
            }

            let _reset = ResetInFlight;
            let result = enumerate_interfaces();
            let _ = tx.send(result);
        });

    if worker.is_err() {
        ENUMERATION_IN_FLIGHT.store(false, Ordering::Release);
        return None;
    }

    match rx.recv_timeout(ENUMERATION_TIMEOUT) {
        Ok(Ok(ips)) if !ips.is_empty() => Some(ips),
        Ok(Ok(_)) | Ok(Err(_)) | Err(_) => None,
    }
}

fn enumerate_interfaces() -> Result<Vec<Ipv4Addr>, String> {
    let ifaces = local_ip_address::list_afinet_netifas().map_err(|error| error.to_string())?;
    let ips = ifaces.into_iter().filter_map(|(_, ip)| match ip {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    });
    Ok(normalize_lan_ips(ips))
}

fn local_lan_ipv4s_from_route() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    for destination in ROUTE_DESTINATIONS {
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        let _ = socket.set_write_timeout(Some(Duration::from_millis(200)));
        if socket.connect(destination).is_err() {
            continue;
        }
        if let Ok(SocketAddr::V4(address)) = socket.local_addr() {
            ips.push(*address.ip());
        }
    }
    normalize_lan_ips(ips)
}

fn cache_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(CACHE_FILE)
}

fn load_cached_ips(config_dir: &Path) -> Option<Vec<Ipv4Addr>> {
    let path = cache_path(config_dir);
    let mut file = File::open(path).ok()?;
    let mut contents = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_CACHE_BYTES + 1)
        .read_to_end(&mut contents)
        .ok()?;
    if contents.len() as u64 > MAX_CACHE_BYTES {
        return None;
    }
    parse_cached_ips(&contents).ok()
}

fn parse_cached_ips(contents: &[u8]) -> Result<Vec<Ipv4Addr>, String> {
    let text = std::str::from_utf8(contents).map_err(|_| "cache is not UTF-8".to_string())?;
    let mut ips = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            return Err("cache contains an empty line".to_string());
        }
        if ips.len() >= MAX_CACHE_IPS {
            return Err("cache contains too many addresses".to_string());
        }
        let ip = line
            .trim()
            .parse::<Ipv4Addr>()
            .map_err(|_| "cache contains an invalid IPv4 address".to_string())?;
        if !is_private_lan(&ip) {
            return Err("cache contains a non-private IPv4 address".to_string());
        }
        ips.push(ip);
    }
    let normalized = normalize_lan_ips(ips);
    if normalized.is_empty() {
        return Err("cache contains no LAN addresses".to_string());
    }
    Ok(normalized)
}

fn persist_cached_ips(config_dir: &Path, ips: &[Ipv4Addr]) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let path = cache_path(config_dir);
    let temp_path = path.with_file_name(format!(".{CACHE_FILE}.tmp-{}", Uuid::new_v4().simple()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path)?;
        for ip in ips {
            writeln!(file, "{ip}")?;
        }
        file.sync_all()?;
        drop(file);
        replace_cache_file(&temp_path, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(unix)]
fn replace_cache_file(temp_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temp_path, path)
}

#[cfg(target_os = "windows")]
fn replace_cache_file(temp_path: &Path, path: &Path) -> io::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        REPLACE_FILE_FLAGS,
    };

    let replacement = windows_wide_path(temp_path);
    let destination = windows_wide_path(path);
    if path.exists() {
        let backup =
            path.with_file_name(format!(".{CACHE_FILE}.backup-{}", Uuid::new_v4().simple()));
        let backup_wide = windows_wide_path(&backup);
        let replaced = unsafe {
            ReplaceFileW(
                PCWSTR(destination.as_ptr()),
                PCWSTR(replacement.as_ptr()),
                PCWSTR(backup_wide.as_ptr()),
                REPLACE_FILE_FLAGS(0),
                None,
                None,
            )
        };
        replaced.map_err(windows_io_error)?;
        let _ = fs::remove_file(backup);
        Ok(())
    } else {
        unsafe {
            MoveFileExW(
                PCWSTR(replacement.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(windows_io_error)
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
fn replace_cache_file(temp_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temp_path, path)
}

#[cfg(target_os = "windows")]
fn windows_wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(target_os = "windows")]
fn windows_io_error(error: windows::core::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lan_ips_filters_private_addresses_and_deduplicates() {
        let actual = normalize_lan_ips([
            Ipv4Addr::new(192, 168, 1, 20),
            Ipv4Addr::new(8, 8, 8, 8),
            Ipv4Addr::new(10, 0, 0, 2),
            Ipv4Addr::new(192, 168, 1, 20),
            Ipv4Addr::new(172, 16, 4, 9),
            Ipv4Addr::new(127, 0, 0, 1),
        ]);

        assert_eq!(
            actual,
            vec![
                Ipv4Addr::new(10, 0, 0, 2),
                Ipv4Addr::new(172, 16, 4, 9),
                Ipv4Addr::new(192, 168, 1, 20),
            ]
        );
    }

    #[test]
    fn parse_cached_ips_rejects_public_or_malformed_entries() {
        assert!(parse_cached_ips(b"8.8.8.8\n").is_err());
        assert!(parse_cached_ips(b"not-an-ip\n").is_err());
        assert!(parse_cached_ips(b"10.0.0.2\n\n10.0.0.3\n").is_err());
    }

    #[test]
    fn parse_cached_ips_accepts_bounded_private_entries() {
        assert_eq!(
            parse_cached_ips(b"192.168.1.20\n10.0.0.2\n192.168.1.20\n").unwrap(),
            vec![Ipv4Addr::new(10, 0, 0, 2), Ipv4Addr::new(192, 168, 1, 20)]
        );
    }

    #[test]
    fn choose_snapshot_prefers_live_then_cache_then_route() {
        let live = vec![Ipv4Addr::new(10, 0, 0, 2)];
        let cached = vec![Ipv4Addr::new(192, 168, 1, 20)];
        let route = vec![Ipv4Addr::new(172, 16, 0, 3)];

        assert_eq!(
            choose_snapshot(Some(live.clone()), Some(cached.clone()), route.clone()),
            LanAddressSnapshot {
                ips: live,
                stale: false,
            }
        );
        assert_eq!(
            choose_snapshot(None, Some(cached.clone()), route.clone()),
            LanAddressSnapshot {
                ips: cached,
                stale: true,
            }
        );
        assert_eq!(
            choose_snapshot(None, None, route.clone()),
            LanAddressSnapshot {
                ips: route,
                stale: false,
            }
        );
        assert_eq!(
            choose_snapshot(None, None, Vec::new()),
            LanAddressSnapshot {
                ips: Vec::new(),
                stale: false,
            }
        );
    }

    #[test]
    fn cache_rejects_more_than_the_maximum_number_of_entries() {
        let contents = (0..=MAX_CACHE_IPS)
            .map(|_| "10.0.0.2")
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_cached_ips(contents.as_bytes()).is_err());
    }

    #[test]
    fn oversized_cache_file_is_rejected_before_parsing() {
        let directory = std::env::temp_dir().join(format!(
            "openless-lan-addresses-oversized-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            cache_path(&directory),
            vec![b'1'; (MAX_CACHE_BYTES + 1) as usize],
        )
        .unwrap();

        assert_eq!(load_cached_ips(&directory), None);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn cached_ips_round_trip_through_atomic_file() {
        let directory = std::env::temp_dir().join(format!(
            "openless-lan-addresses-test-{}",
            Uuid::new_v4().simple()
        ));
        let ips = vec![Ipv4Addr::new(10, 0, 0, 2), Ipv4Addr::new(192, 168, 1, 20)];

        persist_cached_ips(&directory, &ips).unwrap();

        assert_eq!(load_cached_ips(&directory), Some(ips));
        let _ = fs::remove_dir_all(directory);
    }
}
