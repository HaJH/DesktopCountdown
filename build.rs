//! Embeds the application icon into the exe as a Windows resource.
//!
//! `winresource::set_icon` files it under resource id 1, which is both what Explorer shows for
//! the exe and what `tray::Tray` loads at runtime (`Icon::from_resource(ICON_RESOURCE_ID)`) --
//! so the tray icon and the exe icon are one asset, not two.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // The *target*: nothing to embed when the output is not a Windows binary.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_icon();
    }
}

/// Build scripts are compiled for the host, so `winresource` -- declared under
/// `[target.'cfg(windows)'.build-dependencies]` -- exists only on a Windows *host*.
#[cfg(windows)]
fn embed_icon() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    if let Err(e) = res.compile() {
        // A build without the resource compiler still produces a working binary -- just an
        // iconless one -- so warn instead of failing the build.
        println!("cargo:warning=could not embed the application icon: {e}");
    }
}

/// Cross-compiling to Windows from elsewhere (`cargo check --target
/// x86_64-pc-windows-msvc` on macOS, which is how the Windows build is type-checked
/// during the macOS port). There is no `winresource` and no resource compiler here, so
/// the exe comes out iconless. Releases are built on a Windows runner, so this never
/// ships -- but say so rather than let it pass silently.
#[cfg(not(windows))]
fn embed_icon() {
    println!("cargo:warning=cross-compiling to Windows: the application icon is not embedded");
}
