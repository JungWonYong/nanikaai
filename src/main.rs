#![forbid(unsafe_code)]
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

fn configure_platform_env() {
    #[cfg(target_os = "linux")]
    {
        let backend_set = std::env::var_os("WINIT_UNIX_BACKEND")
            .is_some_and(|v| !v.is_empty());
        let has_display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if !backend_set && has_display && desktop.to_ascii_uppercase().contains("GNOME") {
            // GNOME Wayland does not allow global client window positioning; use XWayland cleanly when available.
            std::env::set_var("WINIT_UNIX_BACKEND", "x11");
        }
    }
}

fn main() {
    configure_platform_env();
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");
    if let Err(err) = ai_ukagaka::gui::run(smoke_test) {
        eprintln!("[AI Ukagaka ERROR] {err}");
        std::process::exit(1);
    }
}
