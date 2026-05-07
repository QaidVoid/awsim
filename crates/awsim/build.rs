//! Detects whether the bundled-cert assets are present at build time
//! so `tls.rs` can `include_bytes!` them unconditionally on the
//! upstream awsim build, and gracefully fall back to a generated
//! self-signed cert on a fork that doesn't ship them.
//!
//! When `crates/awsim/assets/aws.qaidvoid.dev/{cert,key}.pem` exist
//! we emit `cargo:rustc-cfg=has_bundled_cert`. The runtime check
//! lives in `tls.rs`.

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
}
