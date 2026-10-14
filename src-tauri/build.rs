fn main() {
    // `cargo build --release` alone still builds a Tauri development app: it
    // loads devUrl instead of embedding frontendDist. Never ship that binary.
    if std::env::var("PROFILE").as_deref() == Ok("release") && tauri_build::is_dev() {
        panic!(
            "Release builds must embed the frontend. Run pnpm desktop:build or \
             cargo tauri build. For a manual Cargo build, run pnpm build first, \
             then cargo build --release --features tauri/custom-protocol."
        );
    }
    tauri_build::build()
}
