//! TLS support for the gateway.
//!
//! Some AWS SDK code paths (Cognito hosted UI, S3 transfer
//! acceleration, the Java SDK's CRT client) hard-require an `https`
//! endpoint. To keep the local-dev story zero-config, AWSim mints a
//! self-signed cert + private key on first boot and caches them so
//! the user can point `AWS_CA_BUNDLE` at a stable path.
//!
//! BYO cert/key is supported via `--tls-cert` / `--tls-key` for users
//! who already trust an organisation CA on their machine.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};

/// Filesystem layout + live `RustlsConfig` for the HTTPS listener.
///
/// `cert_path` is exposed so the startup banner can suggest
/// `AWS_CA_BUNDLE=<cert_path>` - that's the cleanest way to make AWS
/// SDKs trust the listener without touching the system trust store.
pub struct TlsAssets {
    pub cert_path: PathBuf,
    pub config: RustlsConfig,
    pub generated: bool,
}

impl TlsAssets {
    /// Snapshot just the bits the `/_awsim/tls` admin endpoint
    /// surfaces. Cloned so the live `RustlsConfig` (not `Clone`)
    /// can keep moving into the listener owner.
    pub fn admin_info(&self, https_port: u16) -> TlsAdminInfo {
        TlsAdminInfo {
            https_port,
            cert_path: self.cert_path.clone(),
        }
    }
}

/// What the bootstrap script needs to wire up `NODE_EXTRA_CA_CERTS`
/// without any out-of-band knowledge of the awsim install.
///
/// Surfaced via `GET /_awsim/tls` (200 when HTTPS is on, 404 when
/// off). Tooling fetches once per run and stamps the result into
/// the project's env files.
#[derive(Clone, Debug)]
pub struct TlsAdminInfo {
    pub https_port: u16,
    pub cert_path: PathBuf,
}

/// Where AWSim looks for / writes the TLS material.
///
/// `Persistent` means "use these exact paths" (BYO cert) and
/// `Managed` means "create them under this directory if missing".
pub enum CertSource<'a> {
    Byo { cert: &'a Path, key: &'a Path },
    Managed { dir: PathBuf },
}

/// Load existing PEMs or generate a fresh self-signed pair, then
/// build a `RustlsConfig` ready to hand to `axum_server`.
pub async fn load_or_generate(source: CertSource<'_>) -> Result<TlsAssets> {
    // `rustls` 0.23 requires picking a default crypto provider before
    // any `RustlsConfig` is built. We use `ring` (matches the feature
    // flag in Cargo.toml) and silently ignore the "already installed"
    // error so a hot-reload path doesn't blow up.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let (cert_path, key_path, generated) = match source {
        CertSource::Byo { cert, key } => (cert.to_path_buf(), key.to_path_buf(), false),
        CertSource::Managed { dir } => ensure_managed_cert(&dir).await?,
    };

    let config = RustlsConfig::from_pem_file(&cert_path, &key_path)
        .await
        .with_context(|| {
            format!(
                "loading TLS cert/key from {} + {}",
                cert_path.display(),
                key_path.display()
            )
        })?;

    // Banner prints `cert_path` for the user to copy into
    // `AWS_CA_BUNDLE` - make it absolute so the export works
    // regardless of which directory the SDK process is launched in.
    // `std::path::absolute` resolves `.` / `..` segments without
    // touching the FS or following symlinks, so it stays correct on
    // macOS where `/tmp` is a symlink to `/private/tmp`.
    let cert_path = std::path::absolute(&cert_path).unwrap_or(cert_path);

    Ok(TlsAssets {
        cert_path,
        config,
        generated,
    })
}

/// Return `(cert_path, key_path, generated_now)`.
///
/// If both files already exist under `dir`, reuse them. Otherwise
/// mint a fresh self-signed cert good for `localhost`, `*.localhost`
/// (so the per-region SQS / SNS sub-host URLs validate), and the
/// usual loopback addresses.
async fn ensure_managed_cert(dir: &Path) -> Result<(PathBuf, PathBuf, bool)> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("creating TLS cache dir {}", dir.display()))?;

    let cert_path = dir.join("awsim-cert.pem");
    let key_path = dir.join("awsim-key.pem");

    if cert_path.exists() && key_path.exists() {
        return Ok((cert_path, key_path, false));
    }

    let (cert_pem, key_pem) = generate_self_signed()?;

    // Write key first with restrictive perms on Unix so the cert
    // can never appear without a matching key on disk.
    write_secret(&key_path, key_pem.as_bytes()).await?;
    tokio::fs::write(&cert_path, cert_pem.as_bytes())
        .await
        .with_context(|| format!("writing TLS cert to {}", cert_path.display()))?;

    Ok((cert_path, key_path, true))
}

fn generate_self_signed() -> Result<(String, String)> {
    let sans = vec![
        SanType::DnsName("localhost".try_into()?),
        // Wildcard so service-prefixed URLs like
        // `sqs.us-east-1.localhost` and `s3.localhost` validate
        // under the same cert. AWSim mints these URLs itself for
        // SQS queue URLs / SNS topic ARNs / virtual-hosted S3.
        SanType::DnsName("*.localhost".try_into()?),
        SanType::IpAddress(IpAddr::from([127, 0, 0, 1])),
        SanType::IpAddress(IpAddr::from([0, 0, 0, 0])),
        SanType::IpAddress(IpAddr::from([0u16; 8])),
        SanType::IpAddress(IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1])),
    ];

    let mut params = CertificateParams::default();
    params.subject_alt_names = sans;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "AWSim Local CA");
    dn.push(DnType::OrganizationName, "AWSim");
    params.distinguished_name = dn;

    let key_pair = KeyPair::generate().context("generating TLS key pair")?;
    let cert = params
        .self_signed(&key_pair)
        .context("signing self-signed TLS cert")?;

    Ok((cert.pem(), key_pair.serialize_pem()))
}

#[cfg(unix)]
async fn write_secret(path: &Path, data: &[u8]) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .await
        .with_context(|| format!("creating TLS key at {}", path.display()))?;
    file.write_all(data)
        .await
        .with_context(|| format!("writing TLS key to {}", path.display()))?;
    file.flush().await.ok();
    Ok(())
}

#[cfg(not(unix))]
async fn write_secret(path: &Path, data: &[u8]) -> Result<()> {
    tokio::fs::write(path, data)
        .await
        .with_context(|| format!("writing TLS key to {}", path.display()))?;
    Ok(())
}

/// Default cache dir for managed TLS material when no `--data-dir`
/// is set: `$XDG_CACHE_HOME/awsim/tls` (or `$HOME/.cache/awsim/tls`)
/// when a HOME is available, falling back to `$TMPDIR/awsim-tls` on
/// systems without one. The XDG path is stable across reboots so the
/// generated cert + `AWS_CA_BUNDLE` line stay valid until the user
/// removes the directory.
pub fn default_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        let xdg = std::env::var("XDG_CACHE_HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".cache"));
        return xdg.join("awsim").join("tls");
    }
    std::env::temp_dir().join("awsim-tls")
}
