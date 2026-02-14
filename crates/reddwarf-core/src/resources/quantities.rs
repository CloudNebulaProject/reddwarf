use std::collections::HashMap;

/// Resource quantities for nodes and pods
#[derive(Debug, Clone, Default)]
pub struct ResourceQuantities {
    /// CPU in millicores (1000 = 1 core)
    pub cpu_millicores: i64,
    /// Memory in bytes
    pub memory_bytes: i64,
}

impl ResourceQuantities {
    /// Parse CPU string (e.g., "2", "1000m", "0.5")
    pub fn parse_cpu(s: &str) -> Result<i64, String> {
        if let Some(m) = s.strip_suffix('m') {
            // Millicores
            m.parse::<i64>()
                .map_err(|e| format!("Invalid CPU millicore value: {}", e))
        } else if let Ok(cores) = s.parse::<f64>() {
            // Cores as float
            Ok((cores * 1000.0) as i64)
        } else {
            Err(format!("Invalid CPU format: {}", s))
        }
    }

    /// Parse memory string (e.g., "128Mi", "1Gi", "1024")
    pub fn parse_memory(s: &str) -> Result<i64, String> {
        if let Some(num) = s.strip_suffix("Ki") {
            Ok(num.parse::<i64>().map_err(|e| e.to_string())? * 1024)
        } else if let Some(num) = s.strip_suffix("Mi") {
            Ok(num.parse::<i64>().map_err(|e| e.to_string())? * 1024 * 1024)
        } else if let Some(num) = s.strip_suffix("Gi") {
            Ok(num.parse::<i64>().map_err(|e| e.to_string())? * 1024 * 1024 * 1024)
        } else {
            // Plain bytes
            s.parse::<i64>().map_err(|e| e.to_string())
        }
    }

    /// Get CPU and memory from a resource map (k8s-openapi format)
    pub fn from_k8s_resource_map(
        resources: &std::collections::BTreeMap<
            String,
            k8s_openapi::apimachinery::pkg::api::resource::Quantity,
        >,
    ) -> Self {
        let cpu_millicores = resources
            .get("cpu")
            .and_then(|q| Self::parse_cpu(&q.0).ok())
            .unwrap_or(0);

        let memory_bytes = resources
            .get("memory")
            .and_then(|q| Self::parse_memory(&q.0).ok())
            .unwrap_or(0);

        Self {
            cpu_millicores,
            memory_bytes,
        }
    }

    /// Get CPU and memory from a resource map (test format)
    pub fn from_resource_map(resources: &HashMap<String, String>) -> Self {
        let cpu_millicores = resources
            .get("cpu")
            .and_then(|s| Self::parse_cpu(s).ok())
            .unwrap_or(0);

        let memory_bytes = resources
            .get("memory")
            .and_then(|s| Self::parse_memory(s).ok())
            .unwrap_or(0);

        Self {
            cpu_millicores,
            memory_bytes,
        }
    }

    /// Convert millicores to illumos zonecfg `capped-cpu` ncpus value.
    ///
    /// Returns a float string: 500m -> "0.50", 2000m -> "2.00".
    pub fn cpu_as_zone_cap(millicores: i64) -> String {
        format!("{:.2}", millicores as f64 / 1000.0)
    }

    /// Convert bytes to illumos zonecfg `capped-memory` physical value.
    ///
    /// Picks the largest clean unit: G, M, K, or raw bytes.
    /// Uses illumos zonecfg suffixes (G/M/K), NOT K8s (Gi/Mi/Ki).
    pub fn memory_as_zone_cap(bytes: i64) -> String {
        const GIB: i64 = 1024 * 1024 * 1024;
        const MIB: i64 = 1024 * 1024;
        const KIB: i64 = 1024;

        if bytes > 0 && bytes % GIB == 0 {
            format!("{}G", bytes / GIB)
        } else if bytes > 0 && bytes % MIB == 0 {
            format!("{}M", bytes / MIB)
        } else if bytes > 0 && bytes % KIB == 0 {
            format!("{}K", bytes / KIB)
        } else {
            format!("{}", bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu() {
        assert_eq!(ResourceQuantities::parse_cpu("1").unwrap(), 1000);
        assert_eq!(ResourceQuantities::parse_cpu("0.5").unwrap(), 500);
        assert_eq!(ResourceQuantities::parse_cpu("100m").unwrap(), 100);
        assert_eq!(ResourceQuantities::parse_cpu("2").unwrap(), 2000);
    }

    #[test]
    fn test_parse_memory() {
        assert_eq!(ResourceQuantities::parse_memory("1024").unwrap(), 1024);
        assert_eq!(ResourceQuantities::parse_memory("1Ki").unwrap(), 1024);
        assert_eq!(
            ResourceQuantities::parse_memory("128Mi").unwrap(),
            128 * 1024 * 1024
        );
        assert_eq!(
            ResourceQuantities::parse_memory("1Gi").unwrap(),
            1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_cpu_as_zone_cap() {
        assert_eq!(ResourceQuantities::cpu_as_zone_cap(500), "0.50");
        assert_eq!(ResourceQuantities::cpu_as_zone_cap(1000), "1.00");
        assert_eq!(ResourceQuantities::cpu_as_zone_cap(2500), "2.50");
        assert_eq!(ResourceQuantities::cpu_as_zone_cap(100), "0.10");
    }

    #[test]
    fn test_memory_as_zone_cap() {
        // Exact GiB
        assert_eq!(
            ResourceQuantities::memory_as_zone_cap(1024 * 1024 * 1024),
            "1G"
        );
        // Exact MiB
        assert_eq!(
            ResourceQuantities::memory_as_zone_cap(512 * 1024 * 1024),
            "512M"
        );
        // Exact KiB
        assert_eq!(
            ResourceQuantities::memory_as_zone_cap(256 * 1024),
            "256K"
        );
        // 1500 MiB = not a clean GiB, falls to MiB
        assert_eq!(
            ResourceQuantities::memory_as_zone_cap(1500 * 1024 * 1024),
            "1500M"
        );
        // Raw bytes (not aligned)
        assert_eq!(ResourceQuantities::memory_as_zone_cap(1023), "1023");
    }
}
