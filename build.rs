//! Windows: puts the app icon into `shotcraft.exe`, so Explorer, shortcuts, the taskbar and the
//! Start menu show it instead of the default program icon. (The window's own icon is set at run
//! time; the file's icon is a resource in the .exe itself.) Other systems: nothing to do.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/shotcraft.ico");
    // Only when building on Windows for Windows: the resource compiler (`rc.exe`, from the Windows
    // SDK that MSVC builds need anyway) is not there on other systems.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/shotcraft.ico");
        // The file's version details are written too; their names default to the package name,
        // `shotcraft`, and Windows shows them in Task Manager and the file's Properties.
        res.set("FileDescription", "ShotCraft");
        res.set("ProductName", "ShotCraft");
        // A build without the icon is still a working build.
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the app icon in shotcraft.exe: {e}");
        }
    }
}
