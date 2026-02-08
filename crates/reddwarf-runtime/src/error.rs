use miette::Diagnostic;
use thiserror::Error;

/// Runtime error type for zone and container operations
#[derive(Error, Debug, Diagnostic)]
pub enum RuntimeError {
    /// Zone not found
    #[error("Zone not found: {zone_name}")]
    #[diagnostic(
        code(reddwarf::runtime::zone_not_found),
        help("Verify the zone name is correct. Use `list_zones()` to see available zones")
    )]
    ZoneNotFound {
        #[allow(unused)]
        zone_name: String,
    },

    /// Zone already exists
    #[error("Zone already exists: {zone_name}")]
    #[diagnostic(
        code(reddwarf::runtime::zone_already_exists),
        help("Delete the existing zone first with `deprovision()`, or use a different zone name")
    )]
    ZoneAlreadyExists {
        #[allow(unused)]
        zone_name: String,
    },

    /// Zone operation failed
    #[error("Zone operation failed for '{zone_name}': {message}")]
    #[diagnostic(
        code(reddwarf::runtime::zone_operation_failed),
        help("Check zone state with `get_zone_state()`. The zone may need to be in a different state for this operation")
    )]
    ZoneOperationFailed {
        #[allow(unused)]
        zone_name: String,
        #[allow(unused)]
        message: String,
    },

    /// Network error
    #[error("Network operation failed: {message}")]
    #[diagnostic(
        code(reddwarf::runtime::network_error),
        help("Verify network interfaces exist with `dladm show-link`. Check that etherstub/VNIC names are not already in use")
    )]
    NetworkError {
        #[allow(unused)]
        message: String,
    },

    /// ZFS error
    #[error("ZFS operation failed: {message}")]
    #[diagnostic(
        code(reddwarf::runtime::zfs_error),
        help("Verify the parent dataset exists with `zfs list`. Ensure sufficient disk space and proper permissions")
    )]
    ZfsError {
        #[allow(unused)]
        message: String,
    },

    /// Command execution failed
    #[error("Command '{command}' failed with exit code {exit_code}")]
    #[diagnostic(code(reddwarf::runtime::command_failed), help("stderr: {stderr}"))]
    CommandFailed {
        #[allow(unused)]
        command: String,
        #[allow(unused)]
        exit_code: i32,
        #[allow(unused)]
        stderr: String,
    },

    /// Invalid configuration
    #[error("Invalid configuration: {message}")]
    #[diagnostic(code(reddwarf::runtime::invalid_config), help("{suggestion}"))]
    InvalidConfig {
        #[allow(unused)]
        message: String,
        #[allow(unused)]
        suggestion: String,
    },

    /// Invalid state transition
    #[error(
        "Invalid state transition for zone '{zone_name}': cannot transition from {from} to {to}"
    )]
    #[diagnostic(
        code(reddwarf::runtime::invalid_state_transition),
        help("Zone must be in state '{required}' for this operation. Current state is '{from}'")
    )]
    InvalidStateTransition {
        #[allow(unused)]
        zone_name: String,
        #[allow(unused)]
        from: String,
        #[allow(unused)]
        to: String,
        #[allow(unused)]
        required: String,
    },

    /// Unsupported platform
    #[error("Operation not supported on this platform")]
    #[diagnostic(
        code(reddwarf::runtime::unsupported_platform),
        help("This operation requires illumos. Use MockRuntime for testing on other platforms")
    )]
    UnsupportedPlatform,

    /// Core library error
    #[error(transparent)]
    #[diagnostic(transparent)]
    CoreError(#[from] reddwarf_core::ReddwarfError),

    /// Storage error
    #[error(transparent)]
    #[diagnostic(transparent)]
    StorageError(#[from] reddwarf_storage::StorageError),

    /// IP address pool exhausted
    #[error("IPAM pool exhausted: no free addresses in {cidr}")]
    #[diagnostic(
        code(reddwarf::runtime::ipam_pool_exhausted),
        help("Expand the pod CIDR range or delete unused pods to free addresses")
    )]
    IpamPoolExhausted {
        #[allow(unused)]
        cidr: String,
    },

    /// Internal error
    #[error("Internal runtime error: {message}")]
    #[diagnostic(
        code(reddwarf::runtime::internal_error),
        help("This is likely a bug in reddwarf-runtime. Please report it with the full error details")
    )]
    InternalError {
        #[allow(unused)]
        message: String,
    },
}

/// Result type alias for runtime operations
pub type Result<T> = std::result::Result<T, RuntimeError>;

impl RuntimeError {
    pub fn zone_not_found(zone_name: impl Into<String>) -> Self {
        Self::ZoneNotFound {
            zone_name: zone_name.into(),
        }
    }

    pub fn zone_already_exists(zone_name: impl Into<String>) -> Self {
        Self::ZoneAlreadyExists {
            zone_name: zone_name.into(),
        }
    }

    pub fn zone_operation_failed(zone_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ZoneOperationFailed {
            zone_name: zone_name.into(),
            message: message.into(),
        }
    }

    pub fn network_error(message: impl Into<String>) -> Self {
        Self::NetworkError {
            message: message.into(),
        }
    }

    pub fn zfs_error(message: impl Into<String>) -> Self {
        Self::ZfsError {
            message: message.into(),
        }
    }

    pub fn command_failed(
        command: impl Into<String>,
        exit_code: i32,
        stderr: impl Into<String>,
    ) -> Self {
        Self::CommandFailed {
            command: command.into(),
            exit_code,
            stderr: stderr.into(),
        }
    }

    pub fn invalid_config(message: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self::InvalidConfig {
            message: message.into(),
            suggestion: suggestion.into(),
        }
    }

    pub fn invalid_state_transition(
        zone_name: impl Into<String>,
        from: impl Into<String>,
        to: impl Into<String>,
        required: impl Into<String>,
    ) -> Self {
        Self::InvalidStateTransition {
            zone_name: zone_name.into(),
            from: from.into(),
            to: to.into(),
            required: required.into(),
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::InternalError {
            message: message.into(),
        }
    }
}
