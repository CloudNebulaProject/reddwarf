use crate::error::{Result, RuntimeError};
use crate::types::{ZoneInfo, ZoneState};

/// Parse a single line from `zoneadm list -cp` output
///
/// Format: zoneid:zonename:state:zonepath:uuid:brand:ip-type
/// Example: 0:global:running:/:uuid:native:shared
///          -:myzone:installed:/zones/myzone:uuid:lx:excl
pub fn parse_zoneadm_line(line: &str) -> Result<ZoneInfo> {
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() < 7 {
        return Err(RuntimeError::internal_error(format!(
            "Malformed zoneadm output: expected at least 7 colon-delimited fields, got {}. Line: '{}'",
            parts.len(),
            line
        )));
    }

    let zone_id = parts[0].parse::<i32>().ok().filter(|&id| id >= 0);
    let zone_name = parts[1].to_string();
    let state = ZoneState::parse(parts[2]).ok_or_else(|| {
        RuntimeError::internal_error(format!("Unknown zone state: '{}'", parts[2]))
    })?;
    let zonepath = parts[3].to_string();
    let uuid = parts[4].to_string();
    let brand = parts[5].to_string();

    Ok(ZoneInfo {
        zone_name,
        zone_id,
        state,
        zonepath,
        brand,
        uuid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_zoneadm_line() {
        let line = "1:myzone:running:/zones/myzone:abc-123:lx:excl";
        let info = parse_zoneadm_line(line).unwrap();
        assert_eq!(info.zone_name, "myzone");
        assert_eq!(info.zone_id, Some(1));
        assert_eq!(info.state, ZoneState::Running);
        assert_eq!(info.zonepath, "/zones/myzone");
        assert_eq!(info.uuid, "abc-123");
        assert_eq!(info.brand, "lx");
    }

    #[test]
    fn test_parse_unbooted_zone() {
        let line = "-:myzone:installed:/zones/myzone:abc-123:reddwarf:excl";
        let info = parse_zoneadm_line(line).unwrap();
        assert_eq!(info.zone_name, "myzone");
        assert_eq!(info.zone_id, None);
        assert_eq!(info.state, ZoneState::Installed);
    }

    #[test]
    fn test_parse_malformed_line() {
        let line = "bad:data";
        let result = parse_zoneadm_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unknown_state() {
        let line = "-:zone:bogus:/path:uuid:brand:excl";
        let result = parse_zoneadm_line(line);
        assert!(result.is_err());
    }
}
