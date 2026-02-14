use miette::{Context, IntoDiagnostic};
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair};
use std::path::{Path, PathBuf};
use tracing::info;

/// How TLS should be configured for the API server.
#[derive(Debug, Clone)]
pub enum TlsMode {
    /// No TLS — plain HTTP.
    Disabled,
    /// Auto-generate a self-signed CA + server certificate.
    /// Certs are persisted under `data_dir` and reused on restart.
    AutoGenerate {
        data_dir: PathBuf,
        san_entries: Vec<String>,
    },
    /// Use explicitly provided PEM certificate and key files.
    Provided {
        cert_path: PathBuf,
        key_path: PathBuf,
    },
}

/// Resolved TLS key material ready for use by the server.
#[derive(Debug, Clone)]
pub struct TlsMaterial {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    pub ca_pem: Option<Vec<u8>>,
}

/// Resolve TLS material from the given mode.
///
/// - `Disabled` → returns `None`
/// - `AutoGenerate` → checks for existing certs on disk; generates if missing
/// - `Provided` → reads cert/key from the supplied paths
pub fn resolve_tls(mode: &TlsMode) -> miette::Result<Option<TlsMaterial>> {
    match mode {
        TlsMode::Disabled => Ok(None),
        TlsMode::AutoGenerate {
            data_dir,
            san_entries,
        } => {
            let ca_path = data_dir.join("ca.pem");
            let cert_path = data_dir.join("server.pem");
            let key_path = data_dir.join("server-key.pem");

            if ca_path.exists() && cert_path.exists() && key_path.exists() {
                info!("Loading existing TLS certificates from {}", data_dir.display());
                let ca_pem = std::fs::read(&ca_path)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("failed to read CA cert at {}", ca_path.display()))?;
                let cert_pem = std::fs::read(&cert_path)
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        format!("failed to read server cert at {}", cert_path.display())
                    })?;
                let key_pem = std::fs::read(&key_path)
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        format!("failed to read server key at {}", key_path.display())
                    })?;

                Ok(Some(TlsMaterial {
                    cert_pem,
                    key_pem,
                    ca_pem: Some(ca_pem),
                }))
            } else {
                info!(
                    "Auto-generating self-signed TLS certificates in {}",
                    data_dir.display()
                );
                generate_self_signed(data_dir, san_entries).map(Some)
            }
        }
        TlsMode::Provided {
            cert_path,
            key_path,
        } => {
            let cert_pem = std::fs::read(cert_path)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("failed to read TLS cert at {}", cert_path.display())
                })?;
            let key_pem = std::fs::read(key_path)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to read TLS key at {}", key_path.display()))?;

            Ok(Some(TlsMaterial {
                cert_pem,
                key_pem,
                ca_pem: None,
            }))
        }
    }
}

/// Generate a self-signed CA and server certificate, writing PEM files to `data_dir`.
fn generate_self_signed(data_dir: &Path, san_entries: &[String]) -> miette::Result<TlsMaterial> {
    std::fs::create_dir_all(data_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to create TLS directory {}", data_dir.display()))?;

    // --- CA ---
    let ca_key = KeyPair::generate().into_diagnostic().wrap_err("failed to generate CA key pair")?;

    let mut ca_params = CertificateParams::new(vec!["Reddwarf CA".to_string()])
        .into_diagnostic()
        .wrap_err("failed to create CA certificate params")?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    let ca_cert = ca_params
        .self_signed(&ca_key)
        .into_diagnostic()
        .wrap_err("failed to self-sign CA certificate")?;

    // --- Server cert ---
    let server_key = KeyPair::generate()
        .into_diagnostic()
        .wrap_err("failed to generate server key pair")?;

    let mut server_params = CertificateParams::new(san_entries.to_vec())
        .into_diagnostic()
        .wrap_err("failed to create server certificate params")?;
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .into_diagnostic()
        .wrap_err("failed to sign server certificate with CA")?;

    // --- Serialize ---
    let ca_pem = ca_cert.pem();
    let cert_pem = server_cert.pem();
    let key_pem = server_key.serialize_pem();

    // --- Write files ---
    let ca_path = data_dir.join("ca.pem");
    let cert_path = data_dir.join("server.pem");
    let key_path = data_dir.join("server-key.pem");

    std::fs::write(&ca_path, &ca_pem)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to write CA cert to {}", ca_path.display()))?;
    std::fs::write(&cert_path, &cert_pem)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to write server cert to {}", cert_path.display()))?;
    std::fs::write(&key_path, &key_pem)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to write server key to {}", key_path.display()))?;

    info!(
        "TLS certificates written to {}  (ca.pem, server.pem, server-key.pem)",
        data_dir.display()
    );

    Ok(TlsMaterial {
        cert_pem: cert_pem.into_bytes(),
        key_pem: key_pem.into_bytes(),
        ca_pem: Some(ca_pem.into_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_auto_generate_creates_certs() {
        let dir = tempdir().unwrap();
        let tls_dir = dir.path().join("tls");

        let mode = TlsMode::AutoGenerate {
            data_dir: tls_dir.clone(),
            san_entries: vec!["localhost".to_string(), "127.0.0.1".to_string()],
        };

        let material = resolve_tls(&mode).unwrap().expect("should produce material");

        assert!(!material.cert_pem.is_empty());
        assert!(!material.key_pem.is_empty());
        assert!(material.ca_pem.is_some());

        // Verify files were written
        assert!(tls_dir.join("ca.pem").exists());
        assert!(tls_dir.join("server.pem").exists());
        assert!(tls_dir.join("server-key.pem").exists());
    }

    #[test]
    fn test_auto_generate_reuses_existing() {
        let dir = tempdir().unwrap();
        let tls_dir = dir.path().join("tls");

        let mode = TlsMode::AutoGenerate {
            data_dir: tls_dir.clone(),
            san_entries: vec!["localhost".to_string()],
        };

        // First call generates
        let first = resolve_tls(&mode).unwrap().unwrap();

        // Second call loads the same files
        let second = resolve_tls(&mode).unwrap().unwrap();

        assert_eq!(first.cert_pem, second.cert_pem);
        assert_eq!(first.key_pem, second.key_pem);
        assert_eq!(first.ca_pem, second.ca_pem);
    }

    #[test]
    fn test_provided_loads_files() {
        let dir = tempdir().unwrap();
        let tls_dir = dir.path().join("tls");

        // Generate certs first so we have valid PEM to load
        let gen_mode = TlsMode::AutoGenerate {
            data_dir: tls_dir.clone(),
            san_entries: vec!["localhost".to_string()],
        };
        resolve_tls(&gen_mode).unwrap();

        let mode = TlsMode::Provided {
            cert_path: tls_dir.join("server.pem"),
            key_path: tls_dir.join("server-key.pem"),
        };

        let material = resolve_tls(&mode).unwrap().expect("should produce material");
        assert!(!material.cert_pem.is_empty());
        assert!(!material.key_pem.is_empty());
        assert!(material.ca_pem.is_none());
    }

    #[test]
    fn test_provided_missing_file_errors() {
        let mode = TlsMode::Provided {
            cert_path: PathBuf::from("/nonexistent/cert.pem"),
            key_path: PathBuf::from("/nonexistent/key.pem"),
        };

        let result = resolve_tls(&mode);
        assert!(result.is_err());
    }

    #[test]
    fn test_disabled_returns_none() {
        let result = resolve_tls(&TlsMode::Disabled).unwrap();
        assert!(result.is_none());
    }
}
