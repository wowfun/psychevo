fn main() {
    #[cfg(all(
        feature = "native-runtime",
        any(target_os = "macos", target_os = "windows", target_os = "linux")
    ))]
    build_native()
}

#[cfg(all(
    feature = "native-runtime",
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
fn build_native() {
    println!("cargo:rerun-if-changed=capabilities/default.json");
    #[cfg(feature = "wdio-test")]
    println!("cargo:rerun-if-changed=capabilities/wdio.json");

    let attributes =
        tauri_build::Attributes::new().capabilities_path_pattern(capabilities_path_pattern());
    if let Err(error) = tauri_build::try_build(attributes) {
        let error = format!("{error:#}");
        println!("{error}");
        if error.starts_with("unknown field") {
            print!(
                "found an unknown configuration field. This usually means that you are using a CLI version that is newer than `tauri-build` and is incompatible. "
            );
            println!(
                "Please try updating the Rust crates by running `cargo update` in the Tauri app folder."
            );
        }
        std::process::exit(1);
    }
}

#[cfg(all(
    feature = "wdio-test",
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
fn capabilities_path_pattern() -> &'static str {
    "./capabilities/*.json"
}

#[cfg(all(
    feature = "native-runtime",
    not(feature = "wdio-test"),
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
fn capabilities_path_pattern() -> &'static str {
    "./capabilities/default.json"
}
