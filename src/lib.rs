#![doc = include_str!("../README.md")]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::listen;

/// Keyboard keys.
pub mod key;

pub use key::Key;

/// An input event.
#[derive(Debug)]
pub enum Event {
    /// The mouse cursor moved.
    ///
    /// Provides the current x and y position of the cursor on the screen[^1].
    ///
    /// [^1]: If the program is listening to an XWayland server, the "root window"
    /// isn't the entire screen, but rather a bounding box around all windows.
    Motion { x: i32, y: i32 },
    /// A keyboard key was pressed.
    KeyPress {
        key: Key,
        /// The position of the key on the keyboard, represented as the corresponding key on a
        /// QWERTY keyboard.
        keycode: Key,
    },
    /// A keyboard key was released.
    KeyRelease {
        key: Key,
        /// See [`KeyPress::keycode`](Event::KeyPress::keycode).
        keycode: Key,
    },
}

#[derive(thiserror::Error, Debug)]
/// Errors that occur when listening for input events.
pub enum EventError {
    #[error("Failed to get current input configuration")]
    GetState,
    #[error("IO error")]
    IO(
        #[from]
        #[source]
        std::io::Error,
    ),
}

#[derive(thiserror::Error, Debug)]
/// Errors that occur when initializing the input listener.
pub enum ListenError {
    #[error("Platform error")]
    Platform,
    #[error("IO error")]
    IO(
        #[from]
        #[source]
        std::io::Error,
    ),
    #[error("No graphical session detected")]
    NoSession,
}
