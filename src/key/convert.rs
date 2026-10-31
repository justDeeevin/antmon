use crate::{Key, key::PlatformKey};
use xkeysym::Keysym;

/// Match on the given type and convert it to a semantic key. There must be a match arm producing
/// every [`Key`](super::Key) variant. Unmatched input will produce
/// [`Key::Unknown`](super::Key::Unknown) with the given [`PlatformKey`](super::PlatformKey) variant.
macro_rules! key_conv {
    (
        $(#[$attr:meta])*
        $vis:vis $name:ident($platform:ident: $in:ty) {
            $($p:pat => $key:ident,)*
        }
    ) => {
        #[doc = concat!("Convert a native ", stringify!($platform), " keycode to its semantic representation.")]
        $(#[$attr])*
        $vis fn $name(code: $in) -> $crate::key::Key {
            use $crate::key::Key;
            match Key::Super {
                $(Key::$key => {})*
                Key::Unknown(_) => {}
            }
            match code {
                $($p => Key::$key,)*
                #[allow(unreachable_code)]
                code => Key::Unknown($crate::key::PlatformKey::$platform(code))
            }
        }
    }
}

key_conv! {
    #[allow(non_upper_case_globals)]
    pub from_keysym(X11: Keysym) {
        Keysym::Escape => Escape,
        Keysym::F1 => F1,
        Keysym::F2 => F2,
        Keysym::F3 => F3,
        Keysym::F4 => F4,
        Keysym::F5 => F5,
        Keysym::F6 => F6,
        Keysym::F7 => F7,
        Keysym::F8 => F8,
        Keysym::F9 => F9,
        Keysym::F10 => F10,
        Keysym::F11 => F11,
        Keysym::F12 => F12,
        Keysym::Print => PrintScreen,
        Keysym::Scroll_Lock => ScrollLock,
        Keysym::Pause => Pause,
        Keysym::grave | Keysym::asciitilde => Grave,
        Keysym::_1 | Keysym::exclam => One,
        Keysym::_2 | Keysym::at => Two,
        Keysym::_3 | Keysym::numbersign => Three,
        Keysym::_4 | Keysym::dollar => Four,
        Keysym::_5 | Keysym::percent => Five,
        Keysym::_6 | Keysym::asciicircum => Six,
        Keysym::_7 | Keysym::ampersand => Seven,
        Keysym::_8 | Keysym::asterisk => Eight,
        Keysym::_9 | Keysym::parenleft => Nine,
        Keysym::_0 | Keysym::parenright => Zero,
        Keysym::minus | Keysym::underscore  => Minus,
        Keysym::equal | Keysym::plus => Equal,
        Keysym::BackSpace => Backspace,
        Keysym::Insert => Insert,
        Keysym::Home => Home,
        Keysym::Page_Up => PageUp,
        Keysym::Tab | Keysym::ISO_Left_Tab => Tab,
        Keysym::Q | Keysym::q => Q,
        Keysym::W | Keysym::w => W,
        Keysym::E | Keysym::e => E,
        Keysym::R | Keysym::r => R,
        Keysym::T | Keysym::t => T,
        Keysym::Y | Keysym::y => Y,
        Keysym::U | Keysym::u => U,
        Keysym::I | Keysym::i => I,
        Keysym::O | Keysym::o => O,
        Keysym::P | Keysym::p => P,
        Keysym::bracketleft | Keysym::braceleft => LeftBracket,
        Keysym::bracketright | Keysym::braceright => RightBracket,
        Keysym::backslash | Keysym::bar => BackSlash,
        Keysym::Delete => Delete,
        Keysym::End => End,
        Keysym::Page_Down => PageDown,
        Keysym::Caps_Lock => CapsLock,
        Keysym::A | Keysym::a => A,
        Keysym::S | Keysym::s => S,
        Keysym::D | Keysym::d => D,
        Keysym::F | Keysym::f => F,
        Keysym::G | Keysym::g => G,
        Keysym::H | Keysym::h => H,
        Keysym::J | Keysym::j => J,
        Keysym::K | Keysym::k => K,
        Keysym::L | Keysym::l => L,
        Keysym::semicolon | Keysym::colon => Semicolon,
        Keysym::apostrophe | Keysym::quotedbl => Apostrophe,
        Keysym::Return | Keysym::Linefeed => Enter,
        Keysym::Shift_L => LeftShift,
        Keysym::Z | Keysym::z => Z,
        Keysym::X | Keysym::x => X,
        Keysym::C | Keysym::c => C,
        Keysym::V | Keysym::v => V,
        Keysym::B | Keysym::b => B,
        Keysym::N | Keysym::n => N,
        Keysym::M | Keysym::m => M,
        Keysym::comma | Keysym::less => Comma,
        Keysym::period | Keysym::greater => Period,
        Keysym::slash | Keysym::question => Slash,
        Keysym::Shift_R => RightShift,
        Keysym::Control_L => LeftControl,
        Keysym::Super_L => Super,
        Keysym::Alt_L => LeftAlt,
        Keysym::space => Space,
        Keysym::Super_R => RightAlt,
        Keysym::Control_R => RightControl,
        Keysym::Up => Up,
        Keysym::Down => Down,
        Keysym::Left => Left,
        Keysym::Right => Right,
        Keysym::Num_Lock => NumLock,
        Keysym::KP_Divide => NumPadSlash,
        Keysym::KP_Multiply => NumPadAsterisk,
        Keysym::KP_Subtract => NumPadMinus,
        Keysym::KP_7 | Keysym::KP_Home => NumPadSeven,
        Keysym::KP_8 | Keysym::KP_Up => NumPadEight,
        Keysym::KP_9 | Keysym::KP_Page_Up => NumPadNine,
        Keysym::KP_Add => NumPadPlus,
        Keysym::KP_4 | Keysym::KP_Left => NumPadFour,
        Keysym::KP_5 => NumPadFive,
        Keysym::KP_6 | Keysym::KP_Right => NumPadSix,
        Keysym::KP_1 | Keysym::KP_End => NumPadOne,
        Keysym::KP_2 | Keysym::KP_Down => NumPadTwo,
        Keysym::KP_3 | Keysym::KP_Page_Down => NumPadThree,
        Keysym::KP_Enter => NumPadEnter,
        Keysym::KP_0 | Keysym::KP_Insert => NumPadZero,
        Keysym::KP_Decimal | Keysym::KP_Delete => NumPadPeriod,
        Keysym::F13 => F13,
        Keysym::F14 => F14,
        Keysym::F15 => F15,
        Keysym::F16 => F16,
        Keysym::F17 => F17,
        Keysym::F18 => F18,
        Keysym::F19 => F19,
        Keysym::F20 => F20,
        Keysym::F21 => F21,
        Keysym::F22 => F22,
        Keysym::F23 => F23,
        Keysym::F24 => F24,
        Keysym::Menu => Menu,
    }
}

pub fn linux_keycode(code: u16) -> Key {
    match code {
        1..=83 | 87 | 88 | 96..=98 | 100 | 102..=111 | 119 | 125 | 139 | 183..=194 | 210 => {
            let discriminant = code.to_ne_bytes();
            let mut out = [0; std::mem::size_of::<Key>()];
            out[0] = discriminant[0];
            out[1] = discriminant[1];
            unsafe { std::mem::transmute::<[u8; _], Key>(out) }
        }
        _ => Key::Unknown(PlatformKey::Linux(code)),
    }
}
