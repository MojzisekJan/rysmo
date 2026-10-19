//! NativeActivity entry for Android and C entry for an iOS host application.
#[cfg(target_os = "android")]
#[bevy::prelude::bevy_main]
fn main() {
    rysmo_app::run();
}

// Exporting a fixed symbol is the only allowed unsafe attribute in this adapter.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn rysmo_main() {
    rysmo_app::run();
}
