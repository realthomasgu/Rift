//! The wayland client library, the cursor library and xkbcommon are opened by name at run time:
//! the window is a winit window, and winit's wayland backend dlopens all three. There is no system
//! library path on the image, so the binary names them as needed libraries, the way lens, the lock
//! screen and Settings do: the loader finds them through the rpath, and a dlopen by soname then
//! returns the copy that is already loaded.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        for arg in [
            "-Wl,--push-state,--no-as-needed",
            "-lwayland-client",
            "-lwayland-cursor",
            "-lxkbcommon",
            "-Wl,--pop-state",
        ] {
            println!("cargo:rustc-link-arg-bins={arg}");
        }
    }
}
