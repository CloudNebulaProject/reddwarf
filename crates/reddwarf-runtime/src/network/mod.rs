pub mod ipam;
pub mod types;

pub use crate::types::{DirectNicConfig, EtherstubConfig, NetworkMode};
pub use ipam::{CidrConfig, IpAllocation, Ipam};

/// Generate a VNIC name from pod namespace and name
pub fn vnic_name_for_pod(namespace: &str, pod_name: &str) -> String {
    // VNIC names have a max length on illumos, so we truncate and hash
    let combined = format!("{}-{}", namespace, pod_name);
    if combined.len() <= 28 {
        format!("vnic_{}", combined.replace('-', "_"))
    } else {
        // Use a simple hash for long names
        let hash = combined
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        format!("vnic_{:08x}", hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vnic_name_short() {
        let name = vnic_name_for_pod("default", "nginx");
        assert_eq!(name, "vnic_default_nginx");
    }

    #[test]
    fn test_vnic_name_long() {
        let name = vnic_name_for_pod("very-long-namespace-name", "very-long-pod-name-here");
        assert!(name.starts_with("vnic_"));
        assert!(name.len() <= 32);
    }
}
