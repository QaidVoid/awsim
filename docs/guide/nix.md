# Nix

AWSim ships a `flake.nix` with a package build and a development shell.

## Prerequisites

- Nix with flakes enabled. Add `experimental-features = nix-command flakes` to `~/.config/nix/nix.conf`.

## Build

```bash
nix build
```

The binary is at `./result/bin/awsim`.

## Run without Cloning

```bash
nix run github:QaidVoid/awsim
```

## Development Shell

```bash
nix develop
```

The dev shell includes:

- `rustc`, `cargo`, `clippy`, `rustfmt`, `rust-analyzer`
- `pkg-config`, `openssl`
- `bun` (for the UI)
- macOS: `Security` and `SystemConfiguration` frameworks

Once inside the shell, build normally:

```bash
cargo build --release
```

## Flake Outputs

| Output | Description |
|--------|-------------|
| `packages.default` | The `awsim` binary |
| `devShells.default` | Development environment |

The package build uses `rustPlatform.buildRustPackage` with `Cargo.lock` for reproducibility.

## NixOS Module

A NixOS module is not yet included. You can run AWSim as a systemd service using the built package:

```nix
# configuration.nix
{ config, pkgs, ... }:
let
  awsim = (builtins.getFlake "github:QaidVoid/awsim").packages.${pkgs.system}.default;
in {
  systemd.services.awsim = {
    description = "AWSim local AWS emulator";
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      ExecStart = "${awsim}/bin/awsim --data-dir /var/lib/awsim";
      Restart = "on-failure";
      StateDirectory = "awsim";
    };
  };
}
```
