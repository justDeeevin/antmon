use derive_more::Debug;
use xkeysym::Keysym;

/// Convert platform-native key representations to [`Key`]s.
pub mod convert;

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
/// A keyboard key.
pub enum Key {
    Escape = 1,
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
    Enter,
    LeftControl,
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
    /// Also known as backtick
    Grave,
    LeftShift,
    BackSlash,
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
    NumPadAsterisk,
    LeftAlt,
    Space,
    CapsLock,
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
    NumLock,
    ScrollLock,
    /// Alternatively home
    NumPadSeven,
    /// Alternatively up
    NumPadEight,
    /// Alternatively page up
    NumPadNine,
    NumPadMinus,
    /// Alternatively left
    NumPadFour,
    NumPadFive,
    /// Alternatively right
    NumPadSix,
    NumPadPlus,
    /// Alternatively end
    NumPadOne,
    /// Alternatively down
    NumPadTwo,
    /// Alternatively page down
    NumPadThree,
    /// Alternatively insert
    NumPadZero,
    /// Alternatively delete
    NumPadPeriod,
    F11 = 87,
    F12,
    NumPadEnter = 96,
    RightControl,
    NumPadSlash,
    RightAlt = 100,
    Home = 102,
    Up,
    PageUp,
    Left,
    Right,
    End,
    Down,
    PageDown,
    Insert,
    Delete,
    Pause = 119,
    Super = 125,
    Menu = 139,
    F13 = 183,
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
    PrintScreen = 210,
    Unknown(PlatformKey),
    // TODO: possibly more obscure keys.
}

#[derive(Debug, Clone, Copy)]
/// A platform-specific keycode.
///
/// Consumers will usually only encounter the variant corresponding to the platform on which the program
/// is running. However, the [`convert`] functions allow for semantic operations outside of the host
/// platform, so each variant must be accounted for.
pub enum PlatformKey {
    Linux(#[debug("0x{_0:x}")] u16),
    X11(#[debug("0x{:x}", _0.raw())] Keysym),
}
