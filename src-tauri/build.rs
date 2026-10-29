fn main() {
    let target = std::env::var("TARGET").expect("Cargo target is missing");
    println!("cargo:rustc-env=MUSICDECODE_TARGET={target}");
    tauri_build::build()
}
