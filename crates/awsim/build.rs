//! Two responsibilities:
//!
//! 1. Detect whether bundled-cert assets are present so `tls.rs` can
//!    `include_bytes!` them unconditionally on the upstream awsim build,
//!    and gracefully fall back to a generated self-signed cert on a
//!    fork that doesn't ship them. When
//!    `crates/awsim/assets/aws.qaidvoid.dev/{cert,key}.pem` exist we
//!    emit `cargo:rustc-cfg=has_bundled_cert`. The runtime check
//!    lives in `tls.rs`.
//!
//! 2. Stage the SvelteKit static build (`ui/build/` at the workspace
//!    root) into `$OUT_DIR/ui-build/` so `rust-embed` can point at a
//!    path that always exists. Without this, `cargo package --verify`
//!    explodes: the workspace `ui/` tree lives outside the crate
//!    tarball, so `$CARGO_MANIFEST_DIR/../../ui/build` resolves into
//!    `target/package/ui/build` and is missing. Staging into `OUT_DIR`
//!    makes the path stable for both in-tree builds (real assets
//!    copied) and `cargo install` from crates.io (empty dir, runtime
//!    falls through to the "UI not built" placeholder).

use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_bundled_cert)");

    let cert = "assets/aws.qaidvoid.dev/cert.pem";
    let key = "assets/aws.qaidvoid.dev/key.pem";

    println!("cargo:rerun-if-changed={cert}");
    println!("cargo:rerun-if-changed={key}");

    if Path::new(cert).exists() && Path::new(key).exists() {
        println!("cargo:rustc-cfg=has_bundled_cert");
    }

    stage_ui_build();
}

fn stage_ui_build() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let src = Path::new(&manifest_dir).join("../../ui/build");
    let dst = Path::new(&out_dir).join("ui-build");

    // Always recreate the staging dir so removed UI files don't linger
    // across incremental builds.
    if dst.exists() {
        fs::remove_dir_all(&dst).expect("clear ui-build staging dir");
    }
    fs::create_dir_all(&dst).expect("create ui-build staging dir");

    if src.exists() {
        println!("cargo:rerun-if-changed={}", src.display());
        copy_dir_recursive(&src, &dst).expect("copy ui/build into OUT_DIR");
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
