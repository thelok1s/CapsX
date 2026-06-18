// build.rs — Embed Windows VERSIONINFO resource into the PE executable.
//
// Uses the `winres` crate which wraps rc.exe (MSVC) or llvm-rc.
// On non-Windows hosts the compile step is skipped gracefully, so cross-
// compile cargo-check from macOS/Linux still works.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set("ProductName", "CapsX");
        res.set("FileDescription", "CapsLock Keyboard-Layout Switcher for Windows");
        res.set("CompanyName", "CapsX Contributors");
        res.set(
            "LegalCopyright",
            "Based on BarsCaps by Mikhail Svarichevsky. MIT License.",
        );
        // VERSION 0.1.0.0 — high word = major.minor, low word = patch.build
        res.set_version_info(winres::VersionInfo::PRODUCTVERSION, 0x0000_0001_0000_0000);
        res.set_version_info(winres::VersionInfo::FILEVERSION, 0x0000_0001_0000_0000);

        // Best-effort: ignore failure if rc.exe / llvm-rc is not in PATH.
        let _ = res.compile();
    }

    println!("cargo:rerun-if-changed=build.rs");
}
