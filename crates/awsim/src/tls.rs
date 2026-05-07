//! TLS support for the gateway.
//!
//! Three cert sources, in order of preference:
//!
//! 1. **BYO** (`--tls-cert` + `--tls-key`): operator-provided PEMs.
//! 2. **Bundled** (`cfg(has_bundled_cert)`): a publicly-trusted
//!    Let's Encrypt cert for `aws.qaidvoid.dev` (and `*.aws.qaidvoid.dev`)
//!    compiled into the binary. The wildcard A record points to
//!    127.0.0.1, so traffic stays on loopback while browsers / SDKs
//!    see a green-padlock cert with no out-of-band trust setup.
//!    This is the upstream default and matches LocalStack's
//!    `localhost.localstack.cloud` model.
//! 3. **Managed** (fallback): a self-signed cert for `localhost`
//!    auto-generated on first boot and cached on disk. Used when
//!    the upstream bundle is absent (forks that strip
//!    `crates/awsim/assets/aws.qaidvoid.dev/`).

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};

/// PEM-encoded full-chain cert for `aws.qaidvoid.dev` /
/// `*.aws.qaidvoid.dev`, signed by Let's Encrypt. Renewed via the
/// repo's `tls-renew` GitHub Action.
#[cfg(has_bundled_cert)]
const BUNDLED_CERT: &[u8] = include_bytes!("../assets/aws.qaidvoid.dev/cert.pem");

/// Matching private key. Distributing this publicly is operationally
/// safe because the DNS A records are locked to 127.0.0.1 - no
/// remote MITM scenario exists.
#[cfg(has_bundled_cert)]
const BUNDLED_KEY: &[u8] = include_bytes!("../assets/aws.qaidvoid.dev/key.pem");

/// Primary domain the bundled cert advertises. Used for the startup
/// banner and the `/_awsim/tls` admin response so tooling knows
/// which URL to point clients at.
#[cfg(has_bundled_cert)]
pub const BUNDLED_DOMAIN: &str = "aws.qaidvoid.dev";

/// Filesystem layout + live `RustlsConfig` for the HTTPS listener.
///
/// `cert_path` is exposed so the startup banner can suggest
/// `AWS_CA_BUNDLE=<cert_path>` for the rare client that doesn't
/// follow the system trust store. When `public_trust` is `true` the
/// cert chains to a publicly-trusted CA and the env var is purely
/// informational - SDKs that follow the OS root store will trust it
/// out of the box.
pub struct TlsAssets {
    pub cert_path: PathBuf,
    pub config: RustlsConfig,
    pub public_trust: bool,
    pub domain: Option<String>,
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
            public_trust: self.public_trust,
            domain: self.domain.clone(),
        }
    }
}

/// What the bootstrap script needs to wire up `NODE_EXTRA_CA_CERTS`
/// (or skip it entirely) without any out-of-band knowledge of the
/// awsim install.
///
/// Surfaced via `GET /_awsim/tls` (200 when HTTPS is on, 404 when
/// off). Tooling fetches once per run and decides what to write
/// into the project's env files.
#[derive(Clone, Debug)]
pub struct TlsAdminInfo {
    pub https_port: u16,
    pub cert_path: PathBuf,
    /// `true` when the cert chains to a publicly-trusted CA -
    /// clients that follow the system root store don't need
    /// `AWS_CA_BUNDLE` / `NODE_EXTRA_CA_CERTS`.
    pub public_trust: bool,
    /// Primary DNS name on the cert, when known. `None` for
    /// self-signed managed certs and BYO certs we didn't introspect.
    pub domain: Option<String>,
}

/// Where AWSim sources the TLS material.
pub enum CertSource<'a> {
    /// Operator-provided cert / key paths from `--tls-cert` / `--tls-key`.
    Byo { cert: &'a Path, key: &'a Path },
    /// Compiled-in publicly-trusted cert. Materialised under
    /// `dir` on each boot so `/_awsim/tls` can surface a real path.
    #[cfg(has_bundled_cert)]
    Bundled { dir: PathBuf },
    /// Self-signed fallback. Cached under `dir` across runs. Only
    /// constructed on builds without `cfg(has_bundled_cert)` (forks
    /// that strip the publicly-trusted PEMs); silenced otherwise so
    /// upstream builds don't warn.
    #[cfg_attr(has_bundled_cert, allow(dead_code))]
    Managed { dir: PathBuf },
}

/// Resolve the TLS material into a live `RustlsConfig`, generating
/// or materialising on-disk PEMs as needed.
pub async fn load_or_generate(source: CertSource<'_>) -> Result<TlsAssets> {
    // `rustls` 0.23 requires picking a default crypto provider before
    // any `RustlsConfig` is built. We use `ring` (matches the feature
    // flag in Cargo.toml) and silently ignore the "already installed"
    // error so a hot-reload path doesn't blow up.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let resolved = resolve(source).await?;

    let config = RustlsConfig::from_pem_file(&resolved.cert_path, &resolved.key_path)
        .await
        .with_context(|| {
            format!(
                "loading TLS cert/key from {} + {}",
                resolved.cert_path.display(),
                resolved.key_path.display()
            )
        })?;

    // Banner prints `cert_path` for the user to copy into
    // `AWS_CA_BUNDLE` - make it absolute so the export works
    // regardless of which directory the SDK process is launched in.
    // `std::path::absolute` resolves `.` / `..` segments without
    // touching the FS or following symlinks, so it stays correct on
    // macOS where `/tmp` is a symlink to `/private/tmp`.
    let cert_path = std::path::absolute(&resolved.cert_path).unwrap_or(resolved.cert_path);

    Ok(TlsAssets {
        cert_path,
        config,
        public_trust: resolved.public_trust,
        domain: resolved.domain,
        generated: resolved.generated,
    })
}

struct ResolvedSource {
    cert_path: PathBuf,
    key_path: PathBuf,
    public_trust: bool,
    domain: Option<String>,
    generated: bool,
}

async fn resolve(source: CertSource<'_>) -> Result<ResolvedSource> {
    match source {
        CertSource::Byo { cert, key } => Ok(ResolvedSource {
            cert_path: cert.to_path_buf(),
            key_path: key.to_path_buf(),
            // BYO cert may or may not be publicly trusted - we
            // don't introspect. Operators who BYO know what they're
            // doing and set their own trust expectations.
            public_trust: false,
            domain: None,
            generated: false,
        }),
        #[cfg(has_bundled_cert)]
        CertSource::Bundled { dir } => {
            let (cert_path, key_path, generated) = materialise_bundled_cert(&dir).await?;
            Ok(ResolvedSource {
                cert_path,
                key_path,
                public_trust: true,
                domain: Some(BUNDLED_DOMAIN.to_string()),
                generated,
            })
        }
        CertSource::Managed { dir } => {
            let (cert_path, key_path, generated) = ensure_managed_cert(&dir).await?;
            Ok(ResolvedSource {
                cert_path,
                key_path,
                public_trust: false,
                domain: None,
                generated,
            })
        }
    }
}

/// Write the compiled-in PEMs to `dir` so the rest of the loader
/// (and `/_awsim/tls` consumers) have a real on-disk path. Always
/// rewrites - the embedded bytes are the source of truth, and an
/// awsim upgrade with a renewed cert needs to overwrite a stale
/// on-disk copy.
#[cfg(has_bundled_cert)]
async fn materialise_bundled_cert(dir: &Path) -> Result<(PathBuf, PathBuf, bool)> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("creating TLS bundle dir {}", dir.display()))?;

    let cert_path = dir.join("awsim-bundled-cert.pem");
    let key_path = dir.join("awsim-bundled-key.pem");

    let prior = tokio::fs::try_exists(&cert_path).await.unwrap_or(false)
        && tokio::fs::try_exists(&key_path).await.unwrap_or(false);

    write_secret(&key_path, BUNDLED_KEY).await?;
    tokio::fs::write(&cert_path, BUNDLED_CERT)
        .await
        .with_context(|| format!("writing bundled TLS cert to {}", cert_path.display()))?;

    Ok((cert_path, key_path, !prior))
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

/// Default cache dir for managed / bundled TLS material when no
/// `--data-dir` is set: `$XDG_CACHE_HOME/awsim/tls` (or
/// `$HOME/.cache/awsim/tls`) when a HOME is available, falling back
/// to `$TMPDIR/awsim-tls` on systems without one. The XDG path is
/// stable across reboots so the on-disk PEM stays valid until the
/// user removes the directory.
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
