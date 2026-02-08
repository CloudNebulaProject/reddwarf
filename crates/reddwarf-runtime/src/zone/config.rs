use crate::error::Result;
use crate::types::{NetworkMode, ZoneConfig};

/// Generate a zonecfg command file from a ZoneConfig
pub fn generate_zonecfg(config: &ZoneConfig) -> Result<String> {
    let mut lines = Vec::new();

    lines.push("create".to_string());
    lines.push(format!("set brand={}", config.brand));
    lines.push(format!("set zonepath={}", config.zonepath));
    lines.push("set ip-type=exclusive".to_string());

    // Network resource
    let (vnic_name, ip_address, gateway, prefix_len) = match &config.network {
        NetworkMode::Etherstub(cfg) => (
            &cfg.vnic_name,
            &cfg.ip_address,
            &cfg.gateway,
            cfg.prefix_len,
        ),
        NetworkMode::Direct(cfg) => (
            &cfg.vnic_name,
            &cfg.ip_address,
            &cfg.gateway,
            cfg.prefix_len,
        ),
    };
    lines.push("add net".to_string());
    lines.push(format!("set physical={}", vnic_name));
    lines.push(format!("set allowed-address={}/{}", ip_address, prefix_len));
    lines.push(format!("set defrouter={}", gateway));
    lines.push("end".to_string());

    // CPU cap
    if let Some(ref cpu_cap) = config.cpu_cap {
        lines.push("add capped-cpu".to_string());
        lines.push(format!("set ncpus={}", cpu_cap));
        lines.push("end".to_string());
    }

    // Memory cap
    if let Some(ref memory_cap) = config.memory_cap {
        lines.push("add capped-memory".to_string());
        lines.push(format!("set physical={}", memory_cap));
        lines.push("end".to_string());
    }

    // Filesystem mounts
    for mount in &config.fs_mounts {
        lines.push("add fs".to_string());
        lines.push(format!("set dir={}", mount.mountpoint));
        lines.push(format!("set special={}", mount.source));
        lines.push(format!("set type={}", mount.fs_type));
        for opt in &mount.options {
            lines.push(format!("add options {}", opt));
        }
        lines.push("end".to_string());
    }

    lines.push("verify".to_string());
    lines.push("commit".to_string());

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_generate_zonecfg_lx_brand() {
        let config = ZoneConfig {
            zone_name: "test-zone".to_string(),
            brand: ZoneBrand::Lx,
            zonepath: "/zones/test-zone".to_string(),
            network: NetworkMode::Etherstub(EtherstubConfig {
                etherstub_name: "reddwarf0".to_string(),
                vnic_name: "vnic0".to_string(),
                ip_address: "10.0.0.2".to_string(),
                gateway: "10.0.0.1".to_string(),
                prefix_len: 16,
            }),
            storage: ZoneStorageOpts {
                clone_from: None,
                quota: Some("10G".to_string()),
            },
            lx_image_path: Some("/images/ubuntu-22.04.tar.gz".to_string()),
            processes: vec![],
            cpu_cap: Some("2.0".to_string()),
            memory_cap: Some("1G".to_string()),
            fs_mounts: vec![],
        };

        let result = generate_zonecfg(&config).unwrap();
        assert!(result.contains("set brand=lx"));
        assert!(result.contains("set zonepath=/zones/test-zone"));
        assert!(result.contains("set ip-type=exclusive"));
        assert!(result.contains("set physical=vnic0"));
        assert!(result.contains("set allowed-address=10.0.0.2/16"));
        assert!(result.contains("set defrouter=10.0.0.1"));
        assert!(result.contains("set ncpus=2.0"));
        assert!(result.contains("set physical=1G"));
        assert!(result.contains("verify"));
        assert!(result.contains("commit"));
    }

    #[test]
    fn test_generate_zonecfg_custom_brand_with_fs_mounts() {
        let config = ZoneConfig {
            zone_name: "app-zone".to_string(),
            brand: ZoneBrand::Reddwarf,
            zonepath: "/zones/app-zone".to_string(),
            network: NetworkMode::Direct(DirectNicConfig {
                physical_nic: "igb0".to_string(),
                vnic_name: "vnic1".to_string(),
                ip_address: "192.168.1.10".to_string(),
                gateway: "192.168.1.1".to_string(),
                prefix_len: 24,
            }),
            storage: ZoneStorageOpts {
                clone_from: Some("rpool/zones/template@base".to_string()),
                quota: None,
            },
            lx_image_path: None,
            processes: vec![ContainerProcess {
                name: "web".to_string(),
                command: vec!["/usr/bin/node".to_string(), "server.js".to_string()],
                working_dir: Some("/app".to_string()),
                env: vec![("PORT".to_string(), "3000".to_string())],
            }],
            cpu_cap: None,
            memory_cap: Some("512M".to_string()),
            fs_mounts: vec![FsMount {
                source: "/data/app-config".to_string(),
                mountpoint: "/etc/app".to_string(),
                fs_type: "lofs".to_string(),
                options: vec!["ro".to_string()],
            }],
        };

        let result = generate_zonecfg(&config).unwrap();
        assert!(result.contains("set brand=reddwarf"));
        assert!(result.contains("set physical=vnic1"));
        assert!(result.contains("set allowed-address=192.168.1.10/24"));
        assert!(result.contains("set defrouter=192.168.1.1"));
        assert!(result.contains("set physical=512M"));
        assert!(result.contains("add fs"));
        assert!(result.contains("set dir=/etc/app"));
        assert!(result.contains("set special=/data/app-config"));
        assert!(result.contains("set type=lofs"));
        assert!(result.contains("add options ro"));
        // No cpu cap
        assert!(!result.contains("capped-cpu"));
    }
}
