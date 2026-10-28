//! EC2 instances and volumes backed by libvirt virtual machines.
//!
//! Opt-in through `AWSIM_EC2_BACKEND=libvirt`. Each instance is a KVM
//! guest on libvirt's `default` network, booted from a copy-on-write
//! overlay of a base image with a cloud-init seed, and each volume is a
//! sparse qcow2 file. Systems Manager runs commands in the guest through
//! the QEMU guest agent.

#[cfg(test)]
pub(crate) mod fake;
pub mod hypervisor;
pub mod libvirt;
pub(crate) mod machines;
pub mod seed;

use std::path::{Path, PathBuf};

pub use hypervisor::{GuestInfo, Hypervisor, MachineSpec, Power, VmResult};
pub use libvirt::Libvirt;

/// Base image file name the default image map points at.
pub const DEFAULT_IMAGE_FILE: &str = "ubuntu-24.04-minimal-cloudimg-amd64.img";

/// How EC2 runs instances as virtual machines.
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Holds `instances/`, `volumes/` and, by default, `images/`.
    pub dir: PathBuf,
    /// AMI id to base image, in the order given. `*` matches any AMI.
    pub images: Vec<(String, PathBuf)>,
    pub max_vcpus: u32,
    pub max_memory_mib: u64,
    /// How many instances may hold memory at once.
    pub max_instances: usize,
    /// The AWS endpoint guests use. `None` means AWSim's own HTTP port on
    /// the network gateway.
    pub guest_endpoint: Option<String>,
}

impl VmConfig {
    /// Read the configuration from `AWSIM_EC2_*` environment variables.
    /// `Ok(None)` when the backend is not turned on.
    pub fn from_env(data_dir: Option<&Path>) -> Result<Option<Self>, String> {
        Self::from_vars(|name| std::env::var(name).ok(), data_dir)
    }

    fn from_vars(
        var: impl Fn(&str) -> Option<String>,
        data_dir: Option<&Path>,
    ) -> Result<Option<Self>, String> {
        match var("AWSIM_EC2_BACKEND").as_deref() {
            None | Some("") | Some("state") => return Ok(None),
            Some("libvirt") => {}
            Some(other) => {
                return Err(format!(
                    "AWSIM_EC2_BACKEND must be 'state' or 'libvirt', not '{other}'"
                ));
            }
        }
        let dir = match (var("AWSIM_EC2_VM_DIR"), data_dir) {
            (Some(dir), _) if !dir.is_empty() => PathBuf::from(dir),
            (_, Some(data)) => data.join("ec2-vms"),
            _ => {
                return Err(
                    "AWSIM_EC2_BACKEND=libvirt needs AWSIM_EC2_VM_DIR or --data-dir for its disks"
                        .to_string(),
                );
            }
        };
        let images = match var("AWSIM_EC2_IMAGES") {
            Some(map) if !map.trim().is_empty() => parse_image_map(&map)?,
            _ => vec![("*".to_string(), dir.join("images").join(DEFAULT_IMAGE_FILE))],
        };
        let number = |name: &str, default: u64| -> Result<u64, String> {
            match var(name) {
                Some(v) if !v.is_empty() => match v.parse::<u64>() {
                    Ok(n) if n > 0 => Ok(n),
                    _ => Err(format!("{name} must be a positive integer, not '{v}'")),
                },
                _ => Ok(default),
            }
        };
        Ok(Some(Self {
            dir,
            images,
            max_vcpus: number("AWSIM_EC2_VM_MAX_VCPUS", 2)? as u32,
            max_memory_mib: number("AWSIM_EC2_VM_MAX_MEMORY_MIB", 2048)?,
            max_instances: number("AWSIM_EC2_VM_MAX_INSTANCES", 2)? as usize,
            guest_endpoint: var("AWSIM_EC2_GUEST_ENDPOINT").filter(|v| !v.is_empty()),
        }))
    }

    /// The base image an AMI boots from.
    pub fn image_for(&self, image_id: &str) -> Option<&Path> {
        self.images
            .iter()
            .find(|(ami, _)| ami == image_id)
            .or_else(|| self.images.iter().find(|(ami, _)| ami == "*"))
            .map(|(_, path)| path.as_path())
    }

    /// vCPUs and memory for an instance type, after the caps.
    pub fn shape(&self, instance_type: &str) -> (u32, u64) {
        let (vcpus, memory_mib) = type_shape(instance_type);
        (
            vcpus.min(self.max_vcpus),
            memory_mib.min(self.max_memory_mib),
        )
    }
}

fn parse_image_map(map: &str) -> Result<Vec<(String, PathBuf)>, String> {
    map.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|entry| {
            let (ami, path) = entry
                .split_once('=')
                .ok_or_else(|| format!("AWSIM_EC2_IMAGES entry '{entry}' is not ami=path"))?;
            Ok((ami.trim().to_string(), PathBuf::from(path.trim())))
        })
        .collect()
}

/// vCPUs and MiB for an instance type, from its size suffix. Families
/// differ in memory per vCPU, but the caps flatten that for every size a
/// development host can afford, so the suffix is all that is read.
fn type_shape(instance_type: &str) -> (u32, u64) {
    let size = instance_type.rsplit('.').next().unwrap_or_default();
    match size {
        "nano" => (2, 512),
        "micro" => (2, 1024),
        "small" => (2, 2048),
        "medium" => (2, 4096),
        "large" => (2, 8192),
        "xlarge" => (4, 16384),
        _ => match size
            .strip_suffix("xlarge")
            .and_then(|n| n.parse::<u32>().ok())
        {
            Some(n) => (4 * n, 16384 * u64::from(n)),
            None => (2, 2048),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(vars: &[(&str, &str)], data_dir: Option<&Path>) -> Result<Option<VmConfig>, String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        VmConfig::from_vars(|name| vars.get(name).cloned(), data_dir)
    }

    #[test]
    fn off_unless_asked() {
        assert!(config(&[], None).unwrap().is_none());
        assert!(
            config(&[("AWSIM_EC2_BACKEND", "state")], None)
                .unwrap()
                .is_none()
        );
        assert!(config(&[("AWSIM_EC2_BACKEND", "docker")], None).is_err());
    }

    #[test]
    fn needs_a_directory() {
        assert!(config(&[("AWSIM_EC2_BACKEND", "libvirt")], None).is_err());
        let c = config(
            &[("AWSIM_EC2_BACKEND", "libvirt")],
            Some(Path::new("/data")),
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.dir, PathBuf::from("/data/ec2-vms"));
        assert_eq!(
            c.image_for("ami-anything"),
            Some(Path::new(
                "/data/ec2-vms/images/ubuntu-24.04-minimal-cloudimg-amd64.img"
            ))
        );
        assert_eq!(
            (c.max_vcpus, c.max_memory_mib, c.max_instances),
            (2, 2048, 2)
        );
    }

    #[test]
    fn image_map_prefers_exact_ami() {
        let c = config(
            &[
                ("AWSIM_EC2_BACKEND", "libvirt"),
                ("AWSIM_EC2_VM_DIR", "/vm"),
                ("AWSIM_EC2_IMAGES", "ami-1=/i/one.qcow2, *=/i/any.img"),
            ],
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.image_for("ami-1"), Some(Path::new("/i/one.qcow2")));
        assert_eq!(c.image_for("ami-2"), Some(Path::new("/i/any.img")));

        let exact_only = config(
            &[
                ("AWSIM_EC2_BACKEND", "libvirt"),
                ("AWSIM_EC2_VM_DIR", "/vm"),
                ("AWSIM_EC2_IMAGES", "ami-1=/i/one.qcow2"),
            ],
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(exact_only.image_for("ami-2"), None);
        assert!(
            config(
                &[
                    ("AWSIM_EC2_BACKEND", "libvirt"),
                    ("AWSIM_EC2_VM_DIR", "/vm"),
                    ("AWSIM_EC2_IMAGES", "ami-1"),
                ],
                None
            )
            .is_err()
        );
    }

    #[test]
    fn shapes_are_capped() {
        let c = config(
            &[
                ("AWSIM_EC2_BACKEND", "libvirt"),
                ("AWSIM_EC2_VM_DIR", "/vm"),
                ("AWSIM_EC2_VM_MAX_MEMORY_MIB", "1024"),
            ],
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.shape("t3.nano"), (2, 512));
        assert_eq!(c.shape("t3.large"), (2, 1024));
        assert_eq!(c.shape("m5.4xlarge"), (2, 1024));
        assert_eq!(type_shape("m5.4xlarge"), (16, 65536));
        assert_eq!(type_shape("weird"), (2, 2048));
    }
}
