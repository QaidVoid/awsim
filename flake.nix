{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Multi-target Rust toolchain. Targets cover the musl tarballs
        # the release workflow ships; native dev still uses the host
        # target by default.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          targets = [
            "x86_64-unknown-linux-musl"
            "aarch64-unknown-linux-musl"
          ];
        };

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # Cross-toolchains used as the per-target linker / CC for musl
        # cargo builds. Only meaningful on Linux hosts.
        muslX86 = pkgs.pkgsCross.musl64.stdenv.cc;
        muslAarch64 = pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc;
      in
      {
        packages.default = rustPlatform.buildRustPackage {
          pname = "awsim";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ]
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];

          meta = with pkgs.lib; {
            description = "Fully offline, free AWS development environment";
            homepage = "https://github.com/qaidvoid/awsim";
            license = with licenses; [ mit asl20 ];
            mainProgram = "awsim";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            pkg-config
            openssl
            bun
          ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];

          # Per-target linker + CC + AR overrides for `cargo build
          # --target *-unknown-linux-musl`. Without these, nixpkgs' cc
          # wrapper rejects the cross-compile and the link step fails on
          # `gnu_get_libc_version` / DSO-missing errors.
          shellHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=${muslX86}/bin/${muslX86.targetPrefix}cc
            export CC_x86_64_unknown_linux_musl=${muslX86}/bin/${muslX86.targetPrefix}cc
            export AR_x86_64_unknown_linux_musl=${muslX86.bintools}/bin/${muslX86.targetPrefix}ar

            export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=${muslAarch64}/bin/${muslAarch64.targetPrefix}cc
            export CC_aarch64_unknown_linux_musl=${muslAarch64}/bin/${muslAarch64.targetPrefix}cc
            export AR_aarch64_unknown_linux_musl=${muslAarch64.bintools}/bin/${muslAarch64.targetPrefix}ar
          '';

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      });
}
