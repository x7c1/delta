//! Tauri's build step: reads `tauri.conf.json` (with `tauri.linux.conf.json`
//! merged over it on Linux), validates the icons and generates the context
//! `tauri::generate_context!` embeds.

fn main() {
    tauri_build::build();
}
