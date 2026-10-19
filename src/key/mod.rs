use derive_more::Debug;
use std::ffi::c_uint;

/// Convert native platform keycodes to [`Key`]s.
pub mod convert;

#[derive(Debug, Clone, Copy)]
/// A keyboard key.
pub enum Key {
    Escape,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    PrintScreen,
    ScrollLock,
    Pause,
    /// Also known as backtick
    Grave,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Zero,
    Minus,
    Equal,
    Backspace,
    Insert,
    Home,
    PageUp,
    Tab,
    Q,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,
    LeftBracket,
    RightBracket,
    BackSlash,
    Delete,
    End,
    PageDown,
    CapsLock,
    A,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,
    Semicolon,
    Apostrophe,
    Enter,
    LeftShift,
    Z,
    X,
    C,
    V,
    B,
    N,
    M,
    Comma,
    Period,
    Slash,
    RightShift,
    LeftControl,
    Super,
    LeftAlt,
    Space,
    RightAlt,
    RightControl,
    Up,
    Down,
    Left,
    Right,
    NumLock,
    NumPadSlash,
    NumPadAsterisk,
    NumPadMinus,
    /// Alternatively home
    NumPadSeven,
    /// Alternatively up
    NumPadEight,
    /// Alternatively page up
    NumPadNine,
    NumPadPlus,
    /// Alternatively left
    NumPadFour,
    NumPadFive,
    /// Alternatively right
    NumPadSix,
    /// Alternatively end
    NumPadOne,
    /// Alternatively down
    NumPadTwo,
    /// Alternatively page down
    NumPadThree,
    NumPadEnter,
    /// Alternatively insert
    NumPadZero,
    /// Alternatively delete
    NumPadPeriod,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    Menu,
    Unknown(PlatformKey),
    // TODO: possibly more obscure keys.
    // See https://docs.qmk.fm/keycodes_basic
}

#[derive(Debug, Clone, Copy)]
/// A platform-specific keycode.
///
/// Consumers will usually only encounter the variant corresponding to the platform on which the program
/// is running. However, the [`convert`] functions allow for semantic operations outside of the host
/// platform, so each variant must be accounted for.
pub enum PlatformKey {
    Linux(#[debug("0x{_0:x}")] u16),
    X11(#[debug("0x{_0:x}")] c_uint),
}
