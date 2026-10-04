//! Windows: embed the app icon and version info into `cellar.exe`, so
//! Explorer, the taskbar and shortcuts show the Cellar logo. Regenerate
//! `assets/icon.ico` with `cargo run --example render_icons`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !std::path::Path::new("assets/icon.ico").exists() {
        // Lets `render_icons` itself build before the icon exists.
        println!("cargo:warning=assets/icon.ico missing; cellar.exe gets no icon");
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    res.set("ProductName", "Cellar");
    res.set("FileDescription", "Cellar");
    res.compile().expect("embed Windows resources (needs the Windows SDK's rc.exe)");
}
