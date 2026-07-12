//! Embeds the application icon into the exe as a Windows resource.
//!
//! `winresource::set_icon` files it under resource id 1, which is both what Explorer shows for
//! the exe and what `tray::Tray` loads at runtime (`Icon::from_resource(ICON_RESOURCE_ID)`) --
//! so the tray icon and the exe icon are one asset, not two.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            // A build without the resource compiler still produces a working binary -- just an
            // iconless one -- so warn instead of failing the build.
            println!("cargo:warning=could not embed the application icon: {e}");
        }
    }
}
