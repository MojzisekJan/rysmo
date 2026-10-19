//! Native Windows executable icon, using the SDK already required by MSVC builds.
fn main() {
    println!("cargo:rerun-if-changed=brand/favicon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !std::env::var("HOST").unwrap_or_default().contains("windows") {
        println!("cargo:warning=Windows icon resources are embedded on the native Windows build host.");
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let icon = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("brand/favicon.ico");
    let rc = out.join("rysmo.rc");
    let res = out.join("rysmo.res");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", icon.display().to_string().replace('\\', "/"))).unwrap();
    let status = std::process::Command::new("rc.exe")
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg(&rc)
        .status()
        .expect("Windows SDK rc.exe is required; use a Visual Studio Developer PowerShell");
    assert!(status.success(), "Windows icon resource compilation failed");
    println!("cargo:rustc-link-arg-bin=rysmo={}", res.display());
}
