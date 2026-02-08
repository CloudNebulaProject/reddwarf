use crate::error::{Result, RuntimeError};
use reddwarf_storage::KVStore;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tracing::debug;

/// Parsed CIDR configuration
#[derive(Debug, Clone)]
pub struct CidrConfig {
    /// Base network address
    pub network: Ipv4Addr,
    /// CIDR prefix length
    pub prefix_len: u8,
    /// Gateway address (network + 1)
    pub gateway: Ipv4Addr,
    /// First allocatable host address (network + 2)
    pub first_host: Ipv4Addr,
    /// Broadcast address (last in range)
    pub broadcast: Ipv4Addr,
}

/// An allocated IP for a pod
#[derive(Debug, Clone)]
pub struct IpAllocation {
    pub ip_address: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub prefix_len: u8,
}

/// IPAM (IP Address Management) backed by a KVStore
///
/// Storage keys:
/// - `ipam/_cidr` → the CIDR string (e.g. "10.88.0.0/16")
/// - `ipam/alloc/{ip}` → `"{namespace}/{pod_name}"`
pub struct Ipam {
    storage: Arc<dyn KVStore>,
    cidr: CidrConfig,
}

const IPAM_CIDR_KEY: &[u8] = b"ipam/_cidr";
const IPAM_ALLOC_PREFIX: &[u8] = b"ipam/alloc/";

impl Ipam {
    /// Create a new IPAM instance, persisting the CIDR config
    pub fn new(storage: Arc<dyn KVStore>, cidr_str: &str) -> Result<Self> {
        let cidr = parse_cidr(cidr_str)?;

        // Persist the CIDR configuration
        storage.put(IPAM_CIDR_KEY, cidr_str.as_bytes())?;

        debug!(
            "IPAM initialized: network={}, gateway={}, first_host={}, broadcast={}, prefix_len={}",
            cidr.network, cidr.gateway, cidr.first_host, cidr.broadcast, cidr.prefix_len
        );

        Ok(Self { storage, cidr })
    }

    /// Allocate an IP for a pod. Idempotent: returns existing allocation if one exists.
    pub fn allocate(&self, namespace: &str, pod_name: &str) -> Result<IpAllocation> {
        let pod_key = format!("{}/{}", namespace, pod_name);

        // Check if this pod already has an allocation
        let allocations = self.storage.scan(IPAM_ALLOC_PREFIX)?;
        for (key, value) in &allocations {
            let existing_pod = String::from_utf8_lossy(value);
            if existing_pod == pod_key {
                // Parse the IP from the key: "ipam/alloc/{ip}"
                let key_str = String::from_utf8_lossy(key);
                let ip_str = &key_str[IPAM_ALLOC_PREFIX.len()..];
                if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
                    debug!("IPAM: returning existing allocation {} for {}", ip, pod_key);
                    return Ok(IpAllocation {
                        ip_address: ip,
                        gateway: self.cidr.gateway,
                        prefix_len: self.cidr.prefix_len,
                    });
                }
            }
        }

        // Collect already-allocated IPs
        let allocated: std::collections::HashSet<Ipv4Addr> = allocations
            .iter()
            .filter_map(|(key, _)| {
                let key_str = String::from_utf8_lossy(key);
                let ip_str = &key_str[IPAM_ALLOC_PREFIX.len()..];
                ip_str.parse::<Ipv4Addr>().ok()
            })
            .collect();

        // Find next free IP starting from first_host
        let mut candidate = self.cidr.first_host;
        loop {
            if candidate >= self.cidr.broadcast {
                return Err(RuntimeError::IpamPoolExhausted {
                    cidr: format!("{}/{}", self.cidr.network, self.cidr.prefix_len),
                });
            }

            if !allocated.contains(&candidate) {
                // Allocate this IP
                let alloc_key = format!("ipam/alloc/{}", candidate);
                self.storage.put(alloc_key.as_bytes(), pod_key.as_bytes())?;

                debug!("IPAM: allocated {} for {}", candidate, pod_key);
                return Ok(IpAllocation {
                    ip_address: candidate,
                    gateway: self.cidr.gateway,
                    prefix_len: self.cidr.prefix_len,
                });
            }

            candidate = next_ip(candidate);
        }
    }

    /// Release the IP allocated to a pod
    pub fn release(&self, namespace: &str, pod_name: &str) -> Result<Option<Ipv4Addr>> {
        let pod_key = format!("{}/{}", namespace, pod_name);

        let allocations = self.storage.scan(IPAM_ALLOC_PREFIX)?;
        for (key, value) in &allocations {
            let existing_pod = String::from_utf8_lossy(value);
            if existing_pod == pod_key {
                let key_str = String::from_utf8_lossy(key);
                let ip_str = &key_str[IPAM_ALLOC_PREFIX.len()..];
                let ip = ip_str.parse::<Ipv4Addr>().ok();

                self.storage.delete(key)?;
                debug!("IPAM: released {:?} for {}", ip, pod_key);
                return Ok(ip);
            }
        }

        debug!("IPAM: no allocation found for {}", pod_key);
        Ok(None)
    }

    /// Get all current allocations
    pub fn get_all_allocations(&self) -> Result<BTreeMap<Ipv4Addr, String>> {
        let allocations = self.storage.scan(IPAM_ALLOC_PREFIX)?;
        let mut result = BTreeMap::new();

        for (key, value) in &allocations {
            let key_str = String::from_utf8_lossy(key);
            let ip_str = &key_str[IPAM_ALLOC_PREFIX.len()..];
            if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
                result.insert(ip, String::from_utf8_lossy(value).into_owned());
            }
        }

        Ok(result)
    }
}

/// Parse a CIDR string like "10.88.0.0/16" into a CidrConfig
pub fn parse_cidr(cidr_str: &str) -> Result<CidrConfig> {
    let parts: Vec<&str> = cidr_str.split('/').collect();
    if parts.len() != 2 {
        return Err(RuntimeError::invalid_config(
            format!("Invalid CIDR format: '{}'", cidr_str),
            "Use format like '10.88.0.0/16'",
        ));
    }

    let network: Ipv4Addr = parts[0].parse().map_err(|_| {
        RuntimeError::invalid_config(
            format!("Invalid network address: '{}'", parts[0]),
            "Use a valid IPv4 address like '10.88.0.0'",
        )
    })?;

    let prefix_len: u8 = parts[1].parse().map_err(|_| {
        RuntimeError::invalid_config(
            format!("Invalid prefix length: '{}'", parts[1]),
            "Use a number between 0 and 32",
        )
    })?;

    if prefix_len > 32 {
        return Err(RuntimeError::invalid_config(
            format!("Prefix length {} is out of range", prefix_len),
            "Use a number between 0 and 32",
        ));
    }

    let network_u32 = u32::from(network);
    let host_bits = 32 - prefix_len;
    let mask = if prefix_len == 0 {
        0u32
    } else {
        !((1u32 << host_bits) - 1)
    };
    let broadcast_u32 = network_u32 | !mask;

    let gateway = Ipv4Addr::from(network_u32 + 1);
    let first_host = Ipv4Addr::from(network_u32 + 2);
    let broadcast = Ipv4Addr::from(broadcast_u32);

    Ok(CidrConfig {
        network,
        prefix_len,
        gateway,
        first_host,
        broadcast,
    })
}

/// Increment an IPv4 address by one
fn next_ip(ip: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_storage::RedbBackend;
    use tempfile::tempdir;

    fn make_test_ipam(cidr: &str) -> Ipam {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test-ipam.redb");
        let storage = Arc::new(RedbBackend::new(&db_path).unwrap());
        // We need to keep tempdir alive for the duration, but for tests
        // we leak it to avoid dropping the temp dir too early
        std::mem::forget(dir);
        Ipam::new(storage, cidr).unwrap()
    }

    #[test]
    fn test_parse_cidr_valid() {
        let cidr = parse_cidr("10.88.0.0/16").unwrap();
        assert_eq!(cidr.network, Ipv4Addr::new(10, 88, 0, 0));
        assert_eq!(cidr.prefix_len, 16);
        assert_eq!(cidr.gateway, Ipv4Addr::new(10, 88, 0, 1));
        assert_eq!(cidr.first_host, Ipv4Addr::new(10, 88, 0, 2));
        assert_eq!(cidr.broadcast, Ipv4Addr::new(10, 88, 255, 255));
    }

    #[test]
    fn test_parse_cidr_slash24() {
        let cidr = parse_cidr("192.168.1.0/24").unwrap();
        assert_eq!(cidr.network, Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(cidr.gateway, Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(cidr.first_host, Ipv4Addr::new(192, 168, 1, 2));
        assert_eq!(cidr.broadcast, Ipv4Addr::new(192, 168, 1, 255));
    }

    #[test]
    fn test_parse_cidr_invalid() {
        assert!(parse_cidr("not-a-cidr").is_err());
        assert!(parse_cidr("10.88.0.0").is_err());
        assert!(parse_cidr("10.88.0.0/33").is_err());
        assert!(parse_cidr("bad/16").is_err());
    }

    #[test]
    fn test_allocate_sequential() {
        let ipam = make_test_ipam("10.88.0.0/16");

        let alloc1 = ipam.allocate("default", "pod-a").unwrap();
        assert_eq!(alloc1.ip_address, Ipv4Addr::new(10, 88, 0, 2));
        assert_eq!(alloc1.gateway, Ipv4Addr::new(10, 88, 0, 1));
        assert_eq!(alloc1.prefix_len, 16);

        let alloc2 = ipam.allocate("default", "pod-b").unwrap();
        assert_eq!(alloc2.ip_address, Ipv4Addr::new(10, 88, 0, 3));
    }

    #[test]
    fn test_allocate_idempotent() {
        let ipam = make_test_ipam("10.88.0.0/16");

        let alloc1 = ipam.allocate("default", "pod-a").unwrap();
        let alloc2 = ipam.allocate("default", "pod-a").unwrap();
        assert_eq!(alloc1.ip_address, alloc2.ip_address);
    }

    #[test]
    fn test_release_and_reallocate() {
        let ipam = make_test_ipam("10.88.0.0/16");

        let alloc1 = ipam.allocate("default", "pod-a").unwrap();
        let first_ip = alloc1.ip_address;

        // Allocate a second pod
        let _alloc2 = ipam.allocate("default", "pod-b").unwrap();

        // Release first pod
        let released = ipam.release("default", "pod-a").unwrap();
        assert_eq!(released, Some(first_ip));

        // New pod should reuse the freed IP
        let alloc3 = ipam.allocate("default", "pod-c").unwrap();
        assert_eq!(alloc3.ip_address, first_ip);
    }

    #[test]
    fn test_pool_exhaustion() {
        // /30 gives us network .0, gateway .1, one host .2, broadcast .3
        let ipam = make_test_ipam("10.0.0.0/30");

        // First allocation should succeed (.2)
        let alloc = ipam.allocate("default", "pod-a").unwrap();
        assert_eq!(alloc.ip_address, Ipv4Addr::new(10, 0, 0, 2));

        // Second allocation should fail (only .2 is usable, .3 is broadcast)
        let result = ipam.allocate("default", "pod-b");
        assert!(matches!(
            result.unwrap_err(),
            RuntimeError::IpamPoolExhausted { .. }
        ));
    }

    #[test]
    fn test_get_all_allocations() {
        let ipam = make_test_ipam("10.88.0.0/16");

        ipam.allocate("default", "pod-a").unwrap();
        ipam.allocate("kube-system", "pod-b").unwrap();

        let allocs = ipam.get_all_allocations().unwrap();
        assert_eq!(allocs.len(), 2);
        assert_eq!(allocs[&Ipv4Addr::new(10, 88, 0, 2)], "default/pod-a");
        assert_eq!(allocs[&Ipv4Addr::new(10, 88, 0, 3)], "kube-system/pod-b");
    }

    #[test]
    fn test_release_nonexistent() {
        let ipam = make_test_ipam("10.88.0.0/16");
        let released = ipam.release("default", "nonexistent").unwrap();
        assert_eq!(released, None);
    }
}
