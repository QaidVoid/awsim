use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use awsim_core::CommandExit;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use tokio::process::Command;
use tracing::debug;

use super::hypervisor::{GuestInfo, Hypervisor, MachineSpec, Power, VmResult};

const URI: &str = "qemu:///system";
const NETWORK: &str = "default";
/// Every machine AWSim defines is named this followed by the instance id.
const DOMAIN_PREFIX: &str = "awsim-";
/// How long any one tool invocation may take before it is abandoned.
const TOOL_TIMEOUT: Duration = Duration::from_secs(120);
/// Seconds the guest agent gets to answer one command.
const AGENT_TIMEOUT_SECS: &str = "5";

/// Machines on `qemu:///system`, driven through `virsh`, `qemu-img` and an
/// ISO tool. Files live under one directory: `instances/<id>/` holds an
/// instance's root overlay, seed and console log, and `volumes/` holds
/// one qcow2 file per volume.
pub struct Libvirt {
    dir: PathBuf,
    iso_tool: IsoTool,
    gateway: String,
}

#[derive(Debug, Clone, Copy)]
enum IsoTool {
    Mkisofs,
    Genisoimage,
    Xorriso,
}

impl Libvirt {
    /// Check the tools and the network, and answer a hypervisor storing
    /// its files under `dir`.
    pub async fn connect(dir: &Path) -> VmResult<Self> {
        run("qemu-img", &["--version"]).await?;
        let iso_tool = if run("mkisofs", &["-version"]).await.is_ok() {
            IsoTool::Mkisofs
        } else if run("genisoimage", &["-version"]).await.is_ok() {
            IsoTool::Genisoimage
        } else if run("xorriso", &["-version"]).await.is_ok() {
            IsoTool::Xorriso
        } else {
            return Err("no ISO tool found; install mkisofs, genisoimage or xorriso".into());
        };
        let info = virsh(&["net-info", NETWORK]).await?;
        if !info
            .lines()
            .any(|l| l.starts_with("Active:") && l.ends_with("yes"))
        {
            return Err(format!("libvirt network '{NETWORK}' is not active"));
        }
        let xml = virsh(&["net-dumpxml", NETWORK]).await?;
        let gateway = attribute(&xml, "<ip ", "address")
            .ok_or_else(|| format!("libvirt network '{NETWORK}' has no IPv4 address"))?;
        tokio::fs::create_dir_all(dir.join("instances"))
            .await
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        tokio::fs::create_dir_all(dir.join("volumes"))
            .await
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let dir = tokio::fs::canonicalize(dir)
            .await
            .map_err(|e| format!("cannot resolve {}: {e}", dir.display()))?;
        Ok(Self {
            dir,
            iso_tool,
            gateway,
        })
    }

    /// The host's address on the guests' network.
    pub fn gateway(&self) -> &str {
        &self.gateway
    }

    fn instance_dir(&self, instance_id: &str) -> PathBuf {
        self.dir.join("instances").join(instance_id)
    }

    fn volume_path(&self, volume_id: &str) -> PathBuf {
        self.dir.join("volumes").join(format!("{volume_id}.qcow2"))
    }

    async fn agent(&self, instance_id: &str, command: &Value) -> VmResult<Value> {
        let domain = domain_name(instance_id)?;
        let out = virsh(&[
            "qemu-agent-command",
            &domain,
            "--timeout",
            AGENT_TIMEOUT_SECS,
            &command.to_string(),
        ])
        .await?;
        let parsed: Value =
            serde_json::from_str(&out).map_err(|e| format!("guest agent answered {out}: {e}"))?;
        Ok(parsed["return"].clone())
    }

    /// Targets of the machine's disks, with their source files.
    async fn disks(&self, domain: &str) -> VmResult<Vec<(String, String)>> {
        let out = virsh(&["domblklist", domain, "--inactive"]).await?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let mut cols = l.split_whitespace();
                Some((cols.next()?.to_string(), cols.next()?.to_string()))
            })
            .collect())
    }
}

#[async_trait]
impl Hypervisor for Libvirt {
    async fn create(&self, spec: &MachineSpec) -> VmResult<()> {
        let domain = domain_name(&spec.instance_id)?;
        let dir = self.instance_dir(&spec.instance_id);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

        let info = run(
            "qemu-img",
            &["info", "--output=json", &path_str(&spec.base_image)],
        )
        .await?;
        let info: Value = serde_json::from_str(&info).map_err(|e| e.to_string())?;
        let format = info["format"].as_str().unwrap_or("qcow2").to_string();
        let base_bytes = info["virtual-size"].as_u64().unwrap_or(0);
        let root_bytes = spec.root_gib * 1024 * 1024 * 1024;
        if root_bytes < base_bytes {
            return Err(format!(
                "root volume of {} GiB is smaller than the image's {} bytes",
                spec.root_gib, base_bytes
            ));
        }
        let root = dir.join("root.qcow2");
        run(
            "qemu-img",
            &[
                "create",
                "-q",
                "-f",
                "qcow2",
                "-F",
                &format,
                "-b",
                &path_str(&spec.base_image),
                &path_str(&root),
                &format!("{}G", spec.root_gib),
            ],
        )
        .await?;

        write(&dir.join("user-data"), &spec.user_data).await?;
        write(&dir.join("meta-data"), spec.meta_data.as_bytes()).await?;
        let seed = dir.join("seed.iso");
        let iso_args = [
            "-quiet",
            "-output",
            &path_str(&seed),
            "-volid",
            "cidata",
            "-joliet",
            "-rock",
            &path_str(&dir.join("user-data")),
            &path_str(&dir.join("meta-data")),
        ];
        match self.iso_tool {
            IsoTool::Mkisofs => run("mkisofs", &iso_args).await?,
            IsoTool::Genisoimage => run("genisoimage", &iso_args).await?,
            IsoTool::Xorriso => {
                let mut args = vec!["-as", "mkisofs"];
                args.extend(iso_args);
                run("xorriso", &args).await?
            }
        };
        // The seed holds credentials, and the ISO is all the guest needs.
        let _ = tokio::fs::remove_file(dir.join("user-data")).await;
        let _ = tokio::fs::remove_file(dir.join("meta-data")).await;

        let xml = domain_xml(&domain, spec, &root, &seed, &dir.join("console.log"));
        let xml_path = dir.join("domain.xml");
        write(&xml_path, xml.as_bytes()).await?;
        let defined = virsh(&["define", &path_str(&xml_path)]).await;
        let _ = tokio::fs::remove_file(&xml_path).await;
        defined.map(|_| ())
    }

    async fn start(&self, instance_id: &str) -> VmResult<()> {
        virsh(&["start", &domain_name(instance_id)?])
            .await
            .map(|_| ())
    }

    async fn shutdown(&self, instance_id: &str) -> VmResult<()> {
        virsh(&[
            "shutdown",
            &domain_name(instance_id)?,
            "--mode",
            "agent,acpi",
        ])
        .await
        .map(|_| ())
    }

    async fn destroy(&self, instance_id: &str) -> VmResult<()> {
        if self.power(instance_id).await? == Some(Power::On) {
            virsh(&["destroy", &domain_name(instance_id)?]).await?;
        }
        Ok(())
    }

    async fn remove(&self, instance_id: &str) -> VmResult<()> {
        let domain = domain_name(instance_id)?;
        if self.power(instance_id).await?.is_some() {
            self.destroy(instance_id).await?;
            virsh(&["undefine", &domain]).await?;
        }
        match tokio::fs::remove_dir_all(self.instance_dir(instance_id)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot remove files of {instance_id}: {e}")),
        }
    }

    async fn power(&self, instance_id: &str) -> VmResult<Option<Power>> {
        let domain = domain_name(instance_id)?;
        match virsh(&["domstate", &domain]).await {
            Ok(state) => Ok(Some(match state.trim() {
                "shut off" | "crashed" => Power::Off,
                _ => Power::On,
            })),
            Err(e) if e.contains("failed to get domain") || e.contains("Domain not found") => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    async fn resize(&self, instance_id: &str, vcpus: u32, memory_mib: u64) -> VmResult<()> {
        let domain = domain_name(instance_id)?;
        let xml = virsh(&["dumpxml", &domain, "--inactive"]).await?;
        let kib = (memory_mib * 1024).to_string();
        let xml = replace_element(&xml, "vcpu", &vcpus.to_string());
        let xml = replace_element(&xml, "memory", &kib);
        let xml = replace_element(&xml, "currentMemory", &kib);
        let path = self.instance_dir(instance_id).join("domain.xml");
        write(&path, xml.as_bytes()).await?;
        let defined = virsh(&["define", &path_str(&path)]).await;
        let _ = tokio::fs::remove_file(&path).await;
        defined.map(|_| ())
    }

    async fn create_volume(&self, volume_id: &str, size_gib: u64) -> VmResult<()> {
        checked_id(volume_id, "vol-")?;
        run(
            "qemu-img",
            &[
                "create",
                "-q",
                "-f",
                "qcow2",
                &path_str(&self.volume_path(volume_id)),
                &format!("{size_gib}G"),
            ],
        )
        .await
        .map(|_| ())
    }

    async fn delete_volume(&self, volume_id: &str) -> VmResult<()> {
        checked_id(volume_id, "vol-")?;
        match tokio::fs::remove_file(self.volume_path(volume_id)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot delete {volume_id}: {e}")),
        }
    }

    async fn attach_volume(
        &self,
        instance_id: &str,
        volume_id: &str,
        serial: &str,
    ) -> VmResult<()> {
        let domain = domain_name(instance_id)?;
        checked_id(volume_id, "vol-")?;
        let used: Vec<String> = self
            .disks(&domain)
            .await?
            .into_iter()
            .map(|d| d.0)
            .collect();
        let target = ('b'..='z')
            .map(|c| format!("vd{c}"))
            .find(|t| !used.contains(t))
            .ok_or_else(|| format!("{instance_id} has no free disk slot"))?;
        virsh(&[
            "attach-disk",
            &domain,
            &path_str(&self.volume_path(volume_id)),
            &target,
            "--targetbus",
            "virtio",
            "--driver",
            "qemu",
            "--subdriver",
            "qcow2",
            "--serial",
            serial,
            "--persistent",
        ])
        .await
        .map(|_| ())
    }

    async fn detach_volume(&self, instance_id: &str, volume_id: &str) -> VmResult<()> {
        let domain = domain_name(instance_id)?;
        checked_id(volume_id, "vol-")?;
        let source = path_str(&self.volume_path(volume_id));
        let Some((target, _)) = self
            .disks(&domain)
            .await?
            .into_iter()
            .find(|(_, s)| *s == source)
        else {
            return Ok(());
        };
        virsh(&["detach-disk", &domain, &target, "--persistent"])
            .await
            .map(|_| ())
    }

    async fn ping(&self, instance_id: &str) -> bool {
        self.agent(instance_id, &json!({ "execute": "guest-ping" }))
            .await
            .is_ok()
    }

    async fn guest_info(&self, instance_id: &str) -> Option<GuestInfo> {
        let os = self
            .agent(instance_id, &json!({ "execute": "guest-get-osinfo" }))
            .await
            .ok()?;
        let host = self
            .agent(instance_id, &json!({ "execute": "guest-get-host-name" }))
            .await
            .ok()?;
        Some(GuestInfo {
            os_name: os["name"].as_str().unwrap_or_default().to_string(),
            os_version: os["version-id"].as_str().unwrap_or_default().to_string(),
            hostname: host["host-name"].as_str().unwrap_or_default().to_string(),
        })
    }

    async fn address(&self, instance_id: &str) -> Option<String> {
        let domain = domain_name(instance_id).ok()?;
        let out = virsh(&["domifaddr", &domain, "--source", "lease"])
            .await
            .ok()?;
        out.lines()
            .filter(|l| l.contains("ipv4"))
            .find_map(|l| l.split_whitespace().last())
            .and_then(|cidr| cidr.split('/').next())
            .map(str::to_string)
    }

    async fn exec(
        &self,
        instance_id: &str,
        script: &str,
        env: &[String],
        stdin: Option<&[u8]>,
    ) -> VmResult<i64> {
        let mut arguments = json!({
            "path": "/bin/sh",
            "arg": ["-c", script],
            "env": env,
            "capture-output": true,
        });
        if let Some(input) = stdin {
            arguments["input-data"] = json!(STANDARD.encode(input));
        }
        let answer = self
            .agent(
                instance_id,
                &json!({ "execute": "guest-exec", "arguments": arguments }),
            )
            .await?;
        answer["pid"]
            .as_i64()
            .ok_or_else(|| format!("guest-exec answered without a pid: {answer}"))
    }

    async fn exec_status(&self, instance_id: &str, pid: i64) -> VmResult<Option<CommandExit>> {
        let answer = self
            .agent(
                instance_id,
                &json!({ "execute": "guest-exec-status", "arguments": { "pid": pid } }),
            )
            .await?;
        Ok(exec_outcome(&answer))
    }
}

/// A finished `guest-exec-status` answer as a command exit, or `None`
/// while the process runs. A process killed by a signal exits with
/// 128 plus the signal, as a shell reports it.
pub(crate) fn exec_outcome(answer: &Value) -> Option<CommandExit> {
    if answer["exited"].as_bool() != Some(true) {
        return None;
    }
    let decode = |key: &str| {
        answer[key]
            .as_str()
            .and_then(|b| STANDARD.decode(b).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };
    let exit_code = match (answer["exitcode"].as_i64(), answer["signal"].as_i64()) {
        (Some(code), _) => code as i32,
        (None, Some(signal)) => 128 + signal as i32,
        (None, None) => -1,
    };
    Some(CommandExit {
        exit_code,
        stdout: decode("out-data"),
        stderr: decode("err-data"),
    })
}

/// The domain name for an instance, refusing anything that is not an
/// instance id so no call can reach a domain AWSim did not create.
fn domain_name(instance_id: &str) -> VmResult<String> {
    checked_id(instance_id, "i-")?;
    Ok(format!("{DOMAIN_PREFIX}{instance_id}"))
}

fn checked_id(id: &str, prefix: &str) -> VmResult<()> {
    let ok = id
        .strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()));
    if ok {
        Ok(())
    } else {
        Err(format!("'{id}' is not an id AWSim issued"))
    }
}

/// The libvirt definition of one instance's machine.
pub(crate) fn domain_xml(
    domain: &str,
    spec: &MachineSpec,
    root: &Path,
    seed: &Path,
    console: &Path,
) -> String {
    format!(
        "<domain type='kvm'>
  <name>{domain}</name>
  <description>AWSim EC2 instance {instance}</description>
  <memory unit='MiB'>{memory}</memory>
  <currentMemory unit='MiB'>{memory}</currentMemory>
  <vcpu>{vcpus}</vcpu>
  <os><type arch='x86_64' machine='pc'>hvm</type><boot dev='hd'/></os>
  <features><acpi/><apic/></features>
  <cpu mode='host-passthrough'/>
  <clock offset='utc'/>
  <on_poweroff>destroy</on_poweroff>
  <on_reboot>restart</on_reboot>
  <on_crash>destroy</on_crash>
  <devices>
    <disk type='file' device='disk'>
      <driver name='qemu' type='qcow2' discard='unmap'/>
      <source file='{root}'/>
      <target dev='vda' bus='virtio'/>
    </disk>
    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <source file='{seed}'/>
      <target dev='hda' bus='ide'/>
      <readonly/>
    </disk>
    <interface type='network'>
      <source network='{NETWORK}'/>
      <model type='virtio'/>
    </interface>
    <channel type='unix'>
      <target type='virtio' name='org.qemu.guest_agent.0'/>
    </channel>
    <serial type='pty'>
      <log file='{console}' append='off'/>
    </serial>
    <console type='pty'/>
    <rng model='virtio'>
      <backend model='random'>/dev/urandom</backend>
    </rng>
  </devices>
</domain>
",
        instance = xml_text(&spec.instance_id),
        memory = spec.memory_mib,
        vcpus = spec.vcpus,
        root = xml_text(&path_str(root)),
        seed = xml_text(&path_str(seed)),
        console = xml_text(&path_str(console)),
    )
}

fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
}

/// Replace the text of the first `<tag ...>text</tag>` element.
pub(crate) fn replace_element(xml: &str, tag: &str, text: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let Some(start) = xml
        .match_indices(open.as_str())
        .map(|(i, _)| i)
        .find(|&i| matches!(xml.as_bytes().get(i + open.len()), Some(b'>' | b' ')))
    else {
        return xml.to_string();
    };
    let Some(body) = xml[start..].find('>').map(|i| start + i + 1) else {
        return xml.to_string();
    };
    let Some(end) = xml[body..].find(&close).map(|i| body + i) else {
        return xml.to_string();
    };
    format!("{}{text}{}", &xml[..body], &xml[end..])
}

/// The value of `name='...'` on the first element starting with `element`.
fn attribute(xml: &str, element: &str, name: &str) -> Option<String> {
    let start = xml.find(element)?;
    let tag = &xml[start..start + xml[start..].find('>')?];
    for quote in ['\'', '"'] {
        let key = format!("{name}={quote}");
        if let Some(i) = tag.find(&key) {
            let rest = &tag[i + key.len()..];
            return rest.find(quote).map(|j| rest[..j].to_string());
        }
    }
    None
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

async fn write(path: &Path, bytes: &[u8]) -> VmResult<()> {
    tokio::fs::write(path, bytes)
        .await
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

async fn virsh(args: &[&str]) -> VmResult<String> {
    let mut full = vec!["-c", URI, "-q"];
    full.extend_from_slice(args);
    run("virsh", &full).await
}

async fn run(program: &str, args: &[&str]) -> VmResult<String> {
    debug!(program, ?args, "hypervisor command");
    let output = tokio::time::timeout(
        TOOL_TIMEOUT,
        Command::new(program).args(args).kill_on_drop(true).output(),
    )
    .await
    .map_err(|_| format!("{program} timed out"))?
    .map_err(|e| format!("cannot run {program}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("{program} exited with {}", output.status)
        } else {
            err
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> MachineSpec {
        MachineSpec {
            instance_id: "i-0abc1234".to_string(),
            base_image: PathBuf::from("/images/base.img"),
            root_gib: 8,
            vcpus: 2,
            memory_mib: 2048,
            user_data: Vec::new(),
            meta_data: String::new(),
        }
    }

    #[test]
    fn domain_names_only_come_from_issued_ids() {
        assert_eq!(domain_name("i-0abc1234").unwrap(), "awsim-i-0abc1234");
        for bad in [
            "win11",
            "i-",
            "i-xyz",
            "i-12/../../etc",
            "alpinelinux",
            "vol-1234",
        ] {
            assert!(domain_name(bad).is_err(), "{bad} must be refused");
        }
        assert!(checked_id("vol-12ab", "vol-").is_ok());
        assert!(checked_id("vol-../x", "vol-").is_err());
    }

    #[test]
    fn domain_xml_names_disks_network_and_agent_channel() {
        let xml = domain_xml(
            "awsim-i-0abc1234",
            &spec(),
            Path::new("/vm/instances/i-0abc1234/root.qcow2"),
            Path::new("/vm/instances/i-0abc1234/seed.iso"),
            Path::new("/vm/instances/i-0abc1234/console.log"),
        );
        assert!(xml.contains("<name>awsim-i-0abc1234</name>"));
        assert!(xml.contains("<memory unit='MiB'>2048</memory>"));
        assert!(xml.contains("<vcpu>2</vcpu>"));
        assert!(xml.contains("machine='pc'"));
        assert!(xml.contains("<source file='/vm/instances/i-0abc1234/root.qcow2'/>"));
        assert!(xml.contains("<source network='default'/>"));
        assert!(xml.contains("org.qemu.guest_agent.0"));
    }

    #[test]
    fn replace_element_rewrites_vcpu_and_memory() {
        let xml = "<domain>\n  <memory unit='KiB'>2097152</memory>\n  <currentMemory unit='KiB'>2097152</currentMemory>\n  <vcpu placement='static'>2</vcpu>\n</domain>";
        let xml = replace_element(xml, "vcpu", "4");
        let xml = replace_element(&xml, "memory", "1048576");
        let xml = replace_element(&xml, "currentMemory", "1048576");
        assert!(xml.contains("<vcpu placement='static'>4</vcpu>"));
        assert!(xml.contains("<memory unit='KiB'>1048576</memory>"));
        assert!(xml.contains("<currentMemory unit='KiB'>1048576</currentMemory>"));
    }

    #[test]
    fn reads_network_gateway() {
        let xml = "<network>\n  <name>default</name>\n  <ip address='192.168.122.1' netmask='255.255.255.0'>\n</network>";
        assert_eq!(
            attribute(xml, "<ip ", "address").as_deref(),
            Some("192.168.122.1")
        );
    }

    #[test]
    fn exec_outcome_decodes_output_and_signals() {
        let done =
            json!({ "exited": true, "exitcode": 3, "out-data": "aGkK", "err-data": "b29wcwo=" });
        assert_eq!(
            exec_outcome(&done),
            Some(CommandExit {
                exit_code: 3,
                stdout: "hi\n".to_string(),
                stderr: "oops\n".to_string(),
            })
        );
        assert_eq!(exec_outcome(&json!({ "exited": false })), None);
        let killed = exec_outcome(&json!({ "exited": true, "signal": 9 })).unwrap();
        assert_eq!(killed.exit_code, 137);
    }
}
