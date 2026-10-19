fn main() {
    let mut attributes = tauri_build::Attributes::new();
    // Windows/MSVC: tauri-build links its manifest (Common Controls v6) into
    // binaries only, so `cargo test` executables die at load with
    // STATUS_ENTRYPOINT_NOT_FOUND (tauri-apps/tauri#13419). Embed the same
    // manifest into every artifact instead, as Tauri's own example does.
    let target = |key| std::env::var(key).unwrap_or_default();
    if target("CARGO_CFG_TARGET_OS") == "windows" && target("CARGO_CFG_TARGET_ENV") == "msvc" {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest = std::path::Path::new(&target("OUT_DIR")).join("app.manifest");
        std::fs::write(&manifest, COMMON_CONTROLS_V6).expect("write Windows manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}

/// tauri-build's default Windows manifest, unchanged.
const COMMON_CONTROLS_V6: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;
