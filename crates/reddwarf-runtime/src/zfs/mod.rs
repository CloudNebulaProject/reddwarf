pub use crate::types::ZfsConfig;

/// Derive the full dataset path for a zone
pub fn dataset_path(config: &ZfsConfig, zone_name: &str) -> String {
    format!("{}/{}", config.parent_dataset, zone_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dataset_path() {
        let config = ZfsConfig {
            parent_dataset: "rpool/zones".to_string(),
            clone_from: None,
            quota: None,
        };
        assert_eq!(dataset_path(&config, "myzone"), "rpool/zones/myzone");
    }
}
