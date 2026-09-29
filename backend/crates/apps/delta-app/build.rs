//! Tauri's build step: reads `tauri.conf.json`, validates the icons and
//! generates the context `tauri::generate_context!` embeds.

fn main() {
    tauri_build::build();
}
