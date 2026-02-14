use crate::error::RuntimeError;

/// Detected physical resources of the host.
#[derive(Debug, Clone)]
pub struct SystemResources {
    /// Number of logical CPUs.
    pub cpu_count: u32,
    /// Total physical memory in bytes.
    pub total_memory_bytes: u64,
}

/// How much CPU / memory to reserve for system daemons.
#[derive(Debug, Clone)]
pub struct ResourceReservation {
    /// CPU to reserve in millicores (e.g. 100 = 100m).
    pub cpu_millicores: i64,
    /// Memory to reserve in bytes.
    pub memory_bytes: i64,
}

/// Computed node resource budget (capacity minus reservation).
#[derive(Debug, Clone)]
pub struct NodeResources {
    /// Raw hardware capacity.
    pub capacity: SystemResources,
    /// Allocatable CPU after subtracting reservation, in millicores.
    pub allocatable_cpu_millicores: i64,
    /// Allocatable memory after subtracting reservation, in bytes.
    pub allocatable_memory_bytes: u64,
    /// Maximum number of pods this node will accept.
    pub max_pods: u32,
}

/// Detect the host's CPU count and total memory.
///
/// Uses the `sys_info` crate which supports illumos, Linux, and macOS.
pub fn detect_system_resources() -> Result<SystemResources, RuntimeError> {
    let cpu_count = sys_info::cpu_num().map_err(|e| {
        RuntimeError::resource_detection_failed(format!("failed to detect CPU count: {e}"))
    })?;

    let mem = sys_info::mem_info().map_err(|e| {
        RuntimeError::resource_detection_failed(format!("failed to detect memory: {e}"))
    })?;

    // sys_info::mem_info().total is in KiB
    let total_memory_bytes = mem.total * 1024;

    Ok(SystemResources {
        cpu_count,
        total_memory_bytes,
    })
}

/// Detect system resources and compute allocatable values after subtracting
/// the given reservation. Allocatable values are clamped so they never go
/// negative.
pub fn compute_node_resources(
    reservation: &ResourceReservation,
    max_pods: u32,
) -> Result<NodeResources, RuntimeError> {
    let capacity = detect_system_resources()?;

    let capacity_cpu_millicores = capacity.cpu_count as i64 * 1000;
    let allocatable_cpu_millicores =
        (capacity_cpu_millicores - reservation.cpu_millicores).max(0);

    let allocatable_memory_bytes =
        (capacity.total_memory_bytes as i64 - reservation.memory_bytes).max(0) as u64;

    Ok(NodeResources {
        capacity,
        allocatable_cpu_millicores,
        allocatable_memory_bytes,
        max_pods,
    })
}

/// Convert a byte count to the most human-friendly Kubernetes Quantity string.
///
/// Picks the largest clean binary unit: `"16Gi"`, `"7680Mi"`, `"512Ki"`, or
/// raw bytes if nothing divides evenly.
pub fn format_memory_quantity(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;

    if bytes > 0 && bytes % GIB == 0 {
        format!("{}Gi", bytes / GIB)
    } else if bytes > 0 && bytes % MIB == 0 {
        format!("{}Mi", bytes / MIB)
    } else if bytes > 0 && bytes % KIB == 0 {
        format!("{}Ki", bytes / KIB)
    } else {
        format!("{}", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_system_resources() {
        let res = detect_system_resources().expect("detection should succeed in test env");
        assert!(res.cpu_count > 0, "should detect at least 1 CPU");
        assert!(
            res.total_memory_bytes > 0,
            "should detect nonzero memory"
        );
    }

    #[test]
    fn test_format_memory_quantity_gi() {
        assert_eq!(format_memory_quantity(16 * 1024 * 1024 * 1024), "16Gi");
        assert_eq!(format_memory_quantity(1024 * 1024 * 1024), "1Gi");
    }

    #[test]
    fn test_format_memory_quantity_mi() {
        assert_eq!(format_memory_quantity(7680 * 1024 * 1024), "7680Mi");
        assert_eq!(format_memory_quantity(512 * 1024 * 1024), "512Mi");
    }

    #[test]
    fn test_format_memory_quantity_ki() {
        assert_eq!(format_memory_quantity(512 * 1024), "512Ki");
    }

    #[test]
    fn test_format_memory_quantity_raw_bytes() {
        assert_eq!(format_memory_quantity(1023), "1023");
        assert_eq!(format_memory_quantity(0), "0");
    }

    #[test]
    fn test_compute_reserves_subtracted() {
        let reservation = ResourceReservation {
            cpu_millicores: 100,
            memory_bytes: 256 * 1024 * 1024,
        };
        let nr = compute_node_resources(&reservation, 110)
            .expect("should succeed in test env");

        let capacity_cpu_millis = nr.capacity.cpu_count as i64 * 1000;
        assert!(
            nr.allocatable_cpu_millicores < capacity_cpu_millis,
            "allocatable CPU ({}) should be less than capacity ({})",
            nr.allocatable_cpu_millicores,
            capacity_cpu_millis,
        );
        assert!(
            nr.allocatable_memory_bytes < nr.capacity.total_memory_bytes,
            "allocatable memory ({}) should be less than capacity ({})",
            nr.allocatable_memory_bytes,
            nr.capacity.total_memory_bytes,
        );
        assert_eq!(nr.max_pods, 110);
    }

    #[test]
    fn test_reservation_clamp_to_zero() {
        let reservation = ResourceReservation {
            cpu_millicores: i64::MAX,
            memory_bytes: i64::MAX,
        };
        let nr = compute_node_resources(&reservation, 110)
            .expect("should succeed in test env");

        assert_eq!(nr.allocatable_cpu_millicores, 0);
        assert_eq!(nr.allocatable_memory_bytes, 0);
    }
}
