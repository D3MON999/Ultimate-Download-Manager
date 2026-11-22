fn main() {
    // When this build was made (Unix seconds), shown beside the version so each build can be told
    // apart. The script runs again whenever the app's sources change.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=UDM_BUILT={secs}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../src");
    tauri_build::build()
}
