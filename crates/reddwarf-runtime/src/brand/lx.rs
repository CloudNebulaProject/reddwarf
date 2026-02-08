use crate::error::{Result, RuntimeError};
use crate::types::ZoneConfig;

/// Get the install arguments for an LX brand zone
pub fn lx_install_args(config: &ZoneConfig) -> Result<Vec<String>> {
    let image_path = config.lx_image_path.as_ref().ok_or_else(|| {
        RuntimeError::invalid_config(
            "LX brand zone requires an image path",
            "Set `lx_image_path` in ZoneConfig to the path of a Linux rootfs tarball",
        )
    })?;

    Ok(vec!["-s".to_string(), image_path.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_lx_config(image_path: Option<String>) -> ZoneConfig {
        ZoneConfig {
            zone_name: "lx-test".to_string(),
            brand: ZoneBrand::Lx,
            zonepath: "/zones/lx-test".to_string(),
            network: NetworkMode::Etherstub(EtherstubConfig {
                etherstub_name: "stub0".to_string(),
                vnic_name: "vnic0".to_string(),
                ip_address: "10.0.0.2".to_string(),
                gateway: "10.0.0.1".to_string(),
            }),
            zfs: ZfsConfig {
                parent_dataset: "rpool/zones".to_string(),
                clone_from: None,
                quota: None,
            },
            lx_image_path: image_path,
            processes: vec![],
            cpu_cap: None,
            memory_cap: None,
            fs_mounts: vec![],
        }
    }

    #[test]
    fn test_lx_install_args_with_image() {
        let config = make_lx_config(Some("/images/ubuntu.tar.gz".to_string()));
        let args = lx_install_args(&config).unwrap();
        assert_eq!(args, vec!["-s", "/images/ubuntu.tar.gz"]);
    }

    #[test]
    fn test_lx_install_args_missing_image() {
        let config = make_lx_config(None);
        let result = lx_install_args(&config);
        assert!(result.is_err());
    }
}
