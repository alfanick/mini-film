#[cfg(feature = "desktop-app")]
fn main() {
    tauri_build::build();
}

#[cfg(not(feature = "desktop-app"))]
fn main() {}
