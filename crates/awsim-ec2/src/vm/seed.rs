use std::io::Read;

use awsim_core::InstanceCredentials;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Directory in the guest holding AWSim's AWS configuration.
pub const GUEST_DIR: &str = "/etc/awsim";
/// Shared config file in the guest: region and endpoint.
pub const GUEST_CONFIG: &str = "/etc/awsim/config";
/// Shared credentials file in the guest, refreshed through the agent.
pub const GUEST_CREDENTIALS: &str = "/etc/awsim/credentials";

const BOUNDARY: &str = "==AWSIM-USER-DATA-BOUNDARY==";

/// How a guest reaches AWS.
#[derive(Debug, Clone)]
pub struct GuestAws {
    /// The endpoint URL every SDK in the guest is pointed at.
    pub endpoint: String,
    pub region: String,
    /// The host's address on the guests' network, which a host name in
    /// `endpoint` is made to resolve to.
    pub gateway: String,
}

impl GuestAws {
    /// The endpoint's host when it is a name rather than an address.
    fn endpoint_host_name(&self) -> Option<&str> {
        let rest = self
            .endpoint
            .split_once("://")
            .map_or(&*self.endpoint, |r| r.1);
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.starts_with('[') {
            return None;
        }
        let host = authority.rsplit_once(':').map_or(authority, |h| h.0);
        let is_address = host.parse::<std::net::Ipv4Addr>().is_ok();
        (!host.is_empty() && !is_address).then_some(host)
    }

    /// `AWS_CONFIG_FILE` contents for the guest.
    pub fn config_file(&self) -> String {
        format!(
            "[default]\nregion = {}\nendpoint_url = {}\n",
            self.region, self.endpoint
        )
    }

    /// The variables every command run through the agent starts with.
    pub fn command_env(&self, credentials: Option<&InstanceCredentials>) -> Vec<String> {
        let mut env = vec![
            "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
            "HOME=/root".to_string(),
            "USER=root".to_string(),
            "LANG=C.UTF-8".to_string(),
            format!("AWS_CONFIG_FILE={GUEST_CONFIG}"),
            format!("AWS_SHARED_CREDENTIALS_FILE={GUEST_CREDENTIALS}"),
            format!("AWS_ENDPOINT_URL={}", self.endpoint),
            format!("AWS_REGION={}", self.region),
            format!("AWS_DEFAULT_REGION={}", self.region),
        ];
        if let Some(c) = credentials {
            env.push(format!("AWS_ACCESS_KEY_ID={}", c.access_key_id));
            env.push(format!("AWS_SECRET_ACCESS_KEY={}", c.secret_access_key));
            env.push(format!("AWS_SESSION_TOKEN={}", c.session_token));
        }
        env
    }
}

/// `AWS_SHARED_CREDENTIALS_FILE` contents for the guest.
pub fn credentials_file(credentials: &InstanceCredentials) -> String {
    format!(
        "[default]\naws_access_key_id = {}\naws_secret_access_key = {}\naws_session_token = {}\n",
        credentials.access_key_id, credentials.secret_access_key, credentials.session_token
    )
}

/// The cloud-init `meta-data` for an instance.
pub fn meta_data(instance_id: &str) -> String {
    format!("instance-id: {instance_id}\nlocal-hostname: {instance_id}\n")
}

/// The cloud-init `user-data` for an instance: the caller's user data,
/// decoded and decompressed, with AWSim's boot configuration added as a
/// second part that merges into the caller's keys rather than replacing
/// them.
pub fn user_data(
    encoded: Option<&str>,
    aws: &GuestAws,
    credentials: Option<&InstanceCredentials>,
) -> Result<Vec<u8>, String> {
    let ours = awsim_cloud_config(aws, credentials);
    let theirs = match encoded {
        Some(text) if !text.is_empty() => decode_user_data(text)?,
        _ => Vec::new(),
    };

    if theirs.is_empty() {
        return Ok(ours.into_bytes());
    }
    if let Some(boundary) = mime_boundary(&theirs) {
        return splice_part(&theirs, &boundary, &ours);
    }
    // `text/plain` makes cloud-init detect the part's type from its first
    // line, as it does for user data that is not MIME at all.
    let mut doc =
        format!("Content-Type: multipart/mixed; boundary=\"{BOUNDARY}\"\nMIME-Version: 1.0\n\n");
    doc.push_str(&format!(
        "--{BOUNDARY}\nContent-Type: text/plain\nMIME-Version: 1.0\nContent-Transfer-Encoding: base64\n\n"
    ));
    doc.push_str(&wrap(&STANDARD.encode(&theirs)));
    doc.push_str(&format!(
        "\n--{BOUNDARY}\n{}\n--{BOUNDARY}--\n",
        cloud_config_part(&ours)
    ));
    Ok(doc.into_bytes())
}

/// User data as EC2 received it: base64, possibly over gzip.
pub fn decode_user_data(encoded: &str) -> Result<Vec<u8>, String> {
    let compact: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    let raw = STANDARD
        .decode(compact)
        .map_err(|_| "Invalid BASE64 encoding of user data.".to_string())?;
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .map_err(|e| format!("user data is not valid gzip: {e}"))?;
        return Ok(out);
    }
    Ok(raw)
}

fn cloud_config_part(cloud_config: &str) -> String {
    format!("Content-Type: text/cloud-config\nMIME-Version: 1.0\n\n{cloud_config}")
}

/// The boundary of user data that is already a MIME multipart document.
fn mime_boundary(doc: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(doc).ok()?;
    let headers = text.split("\n\n").next()?;
    if !headers
        .to_ascii_lowercase()
        .starts_with("content-type: multipart/")
    {
        return None;
    }
    let at = headers.find("boundary=")? + "boundary=".len();
    let rest = &headers[at..];
    let boundary = match rest.strip_prefix('"') {
        Some(quoted) => &quoted[..quoted.find('"')?],
        None => rest.split([';', '\n', '\r', ' ']).next()?,
    };
    Some(boundary.to_string())
}

/// Add AWSim's part to the end of a caller's MIME document.
fn splice_part(doc: &[u8], boundary: &str, cloud_config: &str) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(doc).map_err(|e| e.to_string())?;
    let closing = format!("--{boundary}--");
    let at = text
        .rfind(&closing)
        .ok_or("MIME user data has no closing boundary")?;
    let mut out = String::with_capacity(text.len() + cloud_config.len() + 128);
    out.push_str(&text[..at]);
    out.push_str(&format!(
        "--{boundary}\n{}\n",
        cloud_config_part(cloud_config)
    ));
    out.push_str(&text[at..]);
    Ok(out.into_bytes())
}

fn wrap(b64: &str) -> String {
    b64.as_bytes()
        .chunks(76)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// AWSim's own boot configuration: one `bootcmd` script, which runs on
/// every boot and before any user script.
fn awsim_cloud_config(aws: &GuestAws, credentials: Option<&InstanceCredentials>) -> String {
    let mut script = String::new();
    script.push_str(&format!(
        "mkdir -p {GUEST_DIR}\ncat > {GUEST_CONFIG} <<'AWSIM_EOF'\n{}AWSIM_EOF\nchmod 0644 {GUEST_CONFIG}\n",
        aws.config_file()
    ));
    // The credentials in the seed are only for the first boot. Later ones
    // keep the file AWSim has refreshed since.
    let credentials = credentials.map(credentials_file).unwrap_or_default();
    script.push_str(&format!(
        "if [ ! -e {GUEST_CREDENTIALS} ]; then\ncat > {GUEST_CREDENTIALS} <<'AWSIM_EOF'\n{credentials}AWSIM_EOF\nchmod 0644 {GUEST_CREDENTIALS}\nfi\n"
    ));
    if let Some(host) = aws.endpoint_host_name() {
        script.push_str(&format!(
            "grep -q ' {host}$' /etc/hosts || echo '{} {host}' >> /etc/hosts\n",
            aws.gateway
        ));
    }
    let vars =
        format!("AWS_CONFIG_FILE={GUEST_CONFIG} AWS_SHARED_CREDENTIALS_FILE={GUEST_CREDENTIALS}");
    script.push_str(&format!(
        "for kv in {vars}; do grep -qx \"$kv\" /etc/environment || echo \"$kv\" >> /etc/environment; done\n\
         mkdir -p /etc/systemd/system.conf.d\n\
         printf '[Manager]\\nDefaultEnvironment={vars}\\n' > /etc/systemd/system.conf.d/awsim.conf\n\
         systemctl set-environment {vars}\n"
    ));
    // Volumes are attached with their EC2 device name as the disk serial,
    // and this links that name, `/dev/sdh` say, to the virtio disk.
    script.push_str(
        "cat > /etc/udev/rules.d/90-awsim-volumes.rules <<'AWSIM_EOF'\n\
         KERNEL==\"vd*[!0-9]\", ATTRS{serial}==\"?*\", SYMLINK+=\"%s{serial}\"\n\
         KERNEL==\"vd*[0-9]\", ATTRS{serial}==\"?*\", SYMLINK+=\"%s{serial}%n\"\n\
         AWSIM_EOF\n\
         udevadm control --reload\n\
         udevadm trigger --subsystem-match=block\n",
    );
    // `--no-block` is required: cloud-init's network stage is ordered
    // before sysinit.target and the agent after it, so waiting on the
    // start deadlocks the boot.
    script.push_str(
        "if ! command -v qemu-ga >/dev/null; then\n\
         if command -v apt-get >/dev/null; then\n\
         apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -yq qemu-guest-agent\n\
         elif command -v dnf >/dev/null; then\n\
         dnf -y install qemu-guest-agent\n\
         fi\n\
         fi\n\
         systemctl start --no-block qemu-guest-agent\n",
    );

    let mut doc = String::from(
        "#cloud-config\nmerge_how:\n  - name: list\n    settings: [append]\n  - name: dict\n    settings: [no_replace, recurse_list]\nbootcmd:\n  - |\n",
    );
    for line in script.lines() {
        doc.push_str("    ");
        doc.push_str(line);
        doc.push('\n');
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn aws(endpoint: &str) -> GuestAws {
        GuestAws {
            endpoint: endpoint.to_string(),
            region: "us-east-1".to_string(),
            gateway: "192.168.122.1".to_string(),
        }
    }

    fn creds() -> InstanceCredentials {
        InstanceCredentials {
            access_key_id: "ASIAEXAMPLE".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: "token".to_string(),
            expiration: "2026-10-28T18:00:00Z".to_string(),
            role_name: "owner".to_string(),
        }
    }

    #[test]
    fn without_user_data_only_awsim_config_remains() {
        let doc = user_data(None, &aws("http://192.168.122.1:4566"), Some(&creds())).unwrap();
        let doc = String::from_utf8(doc).unwrap();
        assert!(doc.starts_with("#cloud-config\n"));
        assert!(doc.contains("aws_access_key_id = ASIAEXAMPLE"));
        assert!(doc.contains("endpoint_url = http://192.168.122.1:4566"));
        assert!(doc.contains("systemctl start --no-block qemu-guest-agent"));
        assert!(
            !doc.contains("/etc/hosts"),
            "an address endpoint needs no alias"
        );
    }

    #[test]
    fn host_name_endpoint_resolves_to_gateway() {
        let doc = user_data(None, &aws("https://aws.qaidvoid.dev:4567"), None).unwrap();
        let doc = String::from_utf8(doc).unwrap();
        assert!(doc.contains("echo '192.168.122.1 aws.qaidvoid.dev' >> /etc/hosts"));
    }

    #[test]
    fn gzip_user_data_is_wrapped_after_decompression() {
        let original = "#cloud-config\nruncmd:\n  - echo hi\n";
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(original.as_bytes()).unwrap();
        let encoded = STANDARD.encode(gz.finish().unwrap());

        let doc = user_data(Some(&encoded), &aws("http://192.168.122.1:4566"), None).unwrap();
        let doc = String::from_utf8(doc).unwrap();
        assert!(doc.starts_with("Content-Type: multipart/mixed;"));
        let part = doc
            .split("Content-Transfer-Encoding: base64\n\n")
            .nth(1)
            .unwrap()
            .split("\n--")
            .next()
            .unwrap();
        let compact: String = part.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(STANDARD.decode(compact).unwrap(), original.as_bytes());
        // AWSim's part comes second, so its merge rules apply to the
        // caller's keys.
        let ours = doc.find("Content-Type: text/cloud-config").unwrap();
        assert!(ours > doc.find("Content-Type: text/plain").unwrap());
        assert!(doc.contains("settings: [append]"));
        assert!(doc.trim_end().ends_with(&format!("--{BOUNDARY}--")));
    }

    #[test]
    fn mime_user_data_gains_a_part() {
        let original = "Content-Type: multipart/mixed; boundary=\"XYZ\"\nMIME-Version: 1.0\n\n--XYZ\nContent-Type: text/x-shellscript\n\n#!/bin/sh\necho hi\n--XYZ--\n";
        let doc = user_data(
            Some(&STANDARD.encode(original)),
            &aws("http://192.168.122.1:4566"),
            None,
        )
        .unwrap();
        let doc = String::from_utf8(doc).unwrap();
        assert!(doc.contains("#!/bin/sh\necho hi\n--XYZ\nContent-Type: text/cloud-config"));
        assert!(doc.trim_end().ends_with("--XYZ--"));
    }

    #[test]
    fn rejects_bad_base64() {
        assert!(decode_user_data("not base64!").is_err());
    }

    #[test]
    fn command_env_carries_credentials_and_endpoint() {
        let env = aws("http://192.168.122.1:4566").command_env(Some(&creds()));
        assert!(env.contains(&"AWS_ACCESS_KEY_ID=ASIAEXAMPLE".to_string()));
        assert!(env.contains(&"AWS_ENDPOINT_URL=http://192.168.122.1:4566".to_string()));
        assert!(env.iter().any(|e| e.starts_with("PATH=")));
    }
}
