# EC2 Virtual Machines

By default an EC2 instance in AWSim is a record: `RunInstances` stores it as `running` and nothing boots. With the libvirt backend turned on, every instance is a real KVM guest, every EBS volume is a disk file attached to it, and Systems Manager Run Command executes `AWS-RunShellScript` inside the guest through the QEMU guest agent.

The backend is opt-in and off by default. With it off, EC2 and SSM behave exactly as they do without it.

## Requirements

- Linux with KVM, and libvirt with access to `qemu:///system` (membership of the `libvirt` group is enough).
- The libvirt `default` network, active. Guests get their address from its DHCP server.
- `virsh`, `qemu-img`, and one of `mkisofs`, `genisoimage` or `xorriso` on `PATH`.
- A cloud image to boot from. AWSim never downloads one.

AWSim checks all of this at startup and refuses to start with a message naming what is missing.

## Turning it on

| Env var | Default | Description |
|---|---|---|
| `AWSIM_EC2_BACKEND` | `state` | `libvirt` boots a virtual machine per instance. `state` keeps instances as records only |
| `AWSIM_EC2_VM_DIR` | `<data-dir>/ec2-vms` | Where instance disks, seeds and volume files live. Required when there is no `--data-dir`. The `libvirt` user must be able to traverse it |
| `AWSIM_EC2_IMAGES` | `*=<vm-dir>/images/ubuntu-24.04-minimal-cloudimg-amd64.img` | AMI id to base image, as `ami-a=/path/a.qcow2,*=/path/any.img`. `*` matches any AMI. A launch whose AMI maps to no existing file fails with `InvalidAMIID.NotFound` |
| `AWSIM_EC2_VM_MAX_VCPUS` | `2` | Upper bound on any instance's vCPUs |
| `AWSIM_EC2_VM_MAX_MEMORY_MIB` | `2048` | Upper bound on any instance's memory |
| `AWSIM_EC2_VM_MAX_INSTANCES` | `2` | How many instances may be `pending`, `running` or `stopping` at once. Beyond it, `RunInstances` and `StartInstances` fail with `InsufficientInstanceCapacity` |
| `AWSIM_EC2_GUEST_ENDPOINT` | `http://<network gateway>:<port>` | The AWS endpoint written into guests. When its host is a name, guests resolve that name to the network gateway |

```bash
AWSIM_EC2_BACKEND=libvirt awsim --data-dir ./data
```

## Base image

The default map expects the Ubuntu 24.04 minimal cloud image, 248 MiB:

```bash
mkdir -p ./data/ec2-vms/images && cd ./data/ec2-vms/images
curl -fLO https://cloud-images.ubuntu.com/minimal/releases/noble/release/ubuntu-24.04-minimal-cloudimg-amd64.img
curl -fLO https://cloud-images.ubuntu.com/minimal/releases/noble/release/SHA256SUMS
grep minimal-cloudimg-amd64.img SHA256SUMS | sha256sum -c
```

Any image with cloud-init and a NoCloud datasource works. Instance types map to vCPUs and memory by their size suffix (`micro` is 2 and 1 GiB, `small` 2 and 2 GiB, `large` 2 and 8 GiB, `xlarge` 4 and 16 GiB), then the caps apply.

The guest needs `qemu-guest-agent`. When the image lacks it, as the Ubuntu minimal image does, AWSim's boot configuration installs it with `apt-get` or `dnf` on first boot, which needs internet access from the guest and adds about 20 seconds. For an offline setup, bake it into the base image once, for example with `virt-customize -a <image> --install qemu-guest-agent`.

## How an instance runs

1. `RunInstances` creates `instances/<id>/root.qcow2`, a copy-on-write overlay over the base image sized to the root block device mapping (8 GiB by default), and `instances/<id>/seed.iso`, a cloud-init NoCloud seed. It defines a libvirt domain named `awsim-<instance-id>` on the `default` network, starts it, and answers `pending`.
2. The seed's user data is the request's `UserData`, decoded and decompressed if it is gzip, followed by a second part with AWSim's own `bootcmd`. That part merges into the caller's cloud-config rather than replacing it.
3. Once the guest agent answers and the DHCP lease has an address, the instance is `running` with that address as `PrivateIpAddress`. On this host that takes about 20 seconds.
4. `StopInstances` asks the guest to shut down and reports `stopping` until the machine is off, then `stopped`. A guest that ignores it for five minutes is powered off. `Force` powers off at once. A guest that powers itself off is reported `stopped`.
5. `StartInstances` applies the current instance type (so `ModifyInstanceAttribute` on a stopped instance resizes it), starts the machine and reports `pending` until the agent answers again.
6. `TerminateInstances` powers the machine off, undefines it and deletes `instances/<id>/`. Attached volumes are detached and kept. Nothing else is touched: AWSim only ever addresses domains named `awsim-i-<hex>` and files under its own directory.

AWSim reconciles instance state with the machines every two seconds. Domains outlive the AWSim process. With `--data-dir`, a restarted AWSim picks its running instances up again.

## Volumes

`CreateVolume` creates a sparse `volumes/<volume-id>.qcow2`. `AttachVolume` hot-plugs it, or adds it to a stopped machine's definition, as a virtio disk whose serial is the device name's last part. A udev rule installed at boot links that serial, so a volume attached at `/dev/sdh` is `/dev/sdh` inside the guest, alongside its kernel name `/dev/vdb`. `DeleteVolume` removes the file.

## Run Command

With the backend on, `SendCommand` with `AWS-RunShellScript` runs the `commands`, joined by newlines, under `/bin/sh` as root through the guest agent, honouring `workingDirectory` and `executionTimeout`. `GetCommandInvocation` and `ListCommandInvocations` report the real status, `ResponseCode`, standard output and standard error. `DescribeInstanceInformation` lists an instance once its agent has answered, `Online` while it keeps answering and `ConnectionLost` otherwise. Sending to an instance that is not online fails with `InvalidInstanceId`.

Other documents, and commands that target tags rather than instance ids, keep the synthetic behaviour.

## AWS access from the guest

An instance launched with an instance profile holds a role session for the profile's role, minted through STS so AWSim resolves it to `assumed-role/<role>/<instance-id>`:

- `/etc/awsim/config` holds `region` and `endpoint_url`, and `/etc/awsim/credentials` holds the session as the `[default]` profile. AWSim replaces the credentials through the guest agent every hour.
- `AWS_CONFIG_FILE` and `AWS_SHARED_CREDENTIALS_FILE` point at them in `/etc/environment` and in the systemd default environment, so the AWS CLI and SDKs find them in shells, services and cloud-init scripts. A user's own `~/.aws` files are shadowed while these are set.
- Every Run Command script additionally receives fresh `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `AWS_ENDPOINT_URL` and `AWS_REGION`.

There is no instance metadata service.

The guest reaches AWSim on the host's address on the `default` network, 192.168.122.1. A host firewall that drops new connections from `virbr*` blocks this, and commands that call AWS then time out while everything else keeps working. With nftables, allow AWSim's ports:

```
iifname "virbr*" tcp dport { 4566, 4567 } accept
```

To use the HTTPS listener and its bundled certificate, set `AWSIM_EC2_GUEST_ENDPOINT=https://aws.qaidvoid.dev:4567`. Guests then map `aws.qaidvoid.dev` to the gateway, and the certificate is publicly trusted.

## Cost per instance

| | Size |
|---|---|
| Base image, shared | 248 MiB |
| Root overlay after first boot with the agent installed from the network | about 360 MiB apparent, about 100 MiB on a compressing filesystem |
| Seed | about 370 KiB |
| Volume | only what the guest writes |
| Memory | the type's memory after the cap, 2 GiB by default, of which an idle Ubuntu minimal guest touches about 450 MiB |

## Cleanup

Terminating an instance removes its domain and files. To find domains left by an AWSim that ran without `--data-dir` and was restarted:

```bash
virsh -c qemu:///system list --all --name | grep '^awsim-i-'
```

Remove one with `virsh destroy` and `virsh undefine`, then delete its directory under `instances/`.

## Running beside another AWSim

Several AWSim processes can share libvirt, since instance ids are random and every domain is named after one. Give each its own `--port`, `--data-dir` and `AWSIM_EC2_VM_DIR`, and set `AWSIM_EC2_VM_MAX_INSTANCES` so that their combined memory fits the host.

## Limitations

- Guests sit on the `default` network whatever subnet or security group the launch names, and their address is the lease address, not one from the subnet.
- Only the root block device mapping's size is honoured at launch. `RebootInstances` does not reboot the machine.
- Linux guests only. `AWS-RunPowerShellScript` and Session Manager are not supported.
- A command past its `executionTimeout` is reported `TimedOut` but keeps running in the guest.
