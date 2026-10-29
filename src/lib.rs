#![cfg_attr(feature = "input-capture-portal", feature(return_type_notation))]
#![doc = include_str!("../README.md")]

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::events;

/// Mouse buttons.
pub mod button;
/// Keyboard keys.
pub mod key;

pub use button::Button;
pub use key::Key;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
/// Whether a button is pressed or released.
pub enum ButtonState {
    Pressed,
    Released,
}

/// An input event.
#[derive(Debug)]
pub enum Event {
    /// The mouse cursor moved.
    ///
    /// Provides the current x and y position of the cursor on the screen[^1].
    ///
    /// [^1]: If the program is listening to an XWayland server, the "root window"
    /// isn't the entire screen, but rather a bounding box around all windows.
    Motion {
        x: i32,
        y: i32,
    },
    /// A keyboard key was pressed/released.
    Key {
        key: Key,
        /// The position of the key on the keyboard, represented as the corresponding key on a
        /// QWERTY keyboard.
        keycode: Key,
        state: ButtonState,
    },
    /// A mouse button was pressed/released.
    Button {
        button: Button,
        state: ButtonState,
    },
    Scroll {
        unit: ScrollUnit,
        dx: f64,
        dy: f64,
    },
}

#[derive(Debug)]
/// Whether a scroll event is in units of mouse wheel detents (or "clicks") or logical pixels.
pub enum ScrollUnit {
    Detent,
    Pixel,
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
}
