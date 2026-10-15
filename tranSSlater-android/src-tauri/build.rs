fn main() {
    // `cargo build --release` alone still builds a Tauri development app: it
    // loads devUrl instead of embedding frontendDist. Never ship that binary.
    //
    // 这条守卫只对 Windows 桌面构建有意义：安卓构建走的是 `tauri android build`，
    // 它自己带 mobile 相关 cfg，`is_dev()` 的取值与桌面不是一回事。
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "android" {
        // NDK r27 defaults to 4 KiB; also support modern 16 KiB Android devices.
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
    }
    if target_os == "windows"
        && std::env::var("PROFILE").as_deref() == Ok("release")
        && tauri_build::is_dev()
    {
        panic!(
            "Release builds must embed the frontend. Run pnpm desktop:build or \
             cargo tauri build. For a manual Cargo build, run pnpm build first, \
             then cargo build --release --features tauri/custom-protocol."
        );
    }
    tauri_build::build()
}
