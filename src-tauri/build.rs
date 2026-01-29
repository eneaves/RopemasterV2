fn main() {
    // Re-run build script if migrations or sqlx offline cache change
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=.sqlx");

    // If the environment variable SQLX_FORCE_OFFLINE is set, propagate SQLX_OFFLINE
    if std::env::var("SQLX_FORCE_OFFLINE").is_ok() {
        println!("cargo:rustc-env=SQLX_OFFLINE=true");
    }

    tauri_build::build()
}
