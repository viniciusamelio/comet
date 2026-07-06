fn main() {
    println!("cargo:rustc-check-cfg=cfg(nightly)");
    println!("cargo:rustc-check-cfg=cfg(broken_fmt)");
    // Set by rust-analyzer itself when analyzing this crate, checked by the
    // `#[crate::async_bound]` codegen attribute (see
    // `core/codegen/src/attribute/async_bound/mod.rs`) — never set by this
    // build script, only ever read from macro-expanded code.
    println!("cargo:rustc-check-cfg=cfg(rust_analyzer)");

    if let Some((version, channel, _)) = version_check::triple() {
        if channel.supports_features() {
            println!("cargo:rustc-cfg=nightly");
        }

        if version.at_least("1.67") && version.at_most("1.68.2") {
            println!("cargo:rustc-cfg=broken_fmt");
        }
    }
}
