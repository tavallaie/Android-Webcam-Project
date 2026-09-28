fn main() {
    tauri_build::build();

    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-arg-bin=awc=-Wl,-rpath,$ORIGIN/../lib:$ORIGIN/../lib/awc");
}
