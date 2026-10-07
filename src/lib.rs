#[cfg(target_os = "linux")]
mod linux;

pub mod key;

use key::Key;

pub enum Event {
    Motion { x: i32, y: i32 },
    KeyPress(Key),
    KeyRelease(Key),
}

pub enum Platform {
    Mac,
    Windows,
    X11,
    Wayland,
}
