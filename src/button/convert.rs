/// Match on the given type and convert it to a semantic button. There must be a match arm producing
/// every [`Button`](super::Button) variant. Unmatched input will produce
/// [`Button::Other`](super::Button::Other).
macro_rules! button_conv {
    (
        $(#[$attr:meta])*
        $vis:vis $name:ident($platform:ident: $in:ty) {
            $($p:pat => $button:ident,)*
        }
    ) => {
        #[doc = concat!("Convert a native ", stringify!($platform), " mouse button to its semantic representation.")]
        $(#[$attr])*
        $vis fn $name(code: $in) -> $crate::button::Button {
            use $crate::button::Button;
            match Button::Left {
                $(Button::$button => {})*
                Button::Other(_) => {}
            }
            match code {
                $($p => Button::$button,)*
                #[allow(unreachable_code)]
                code => Button::Other(code as u8)
            }
        }
    }
}

button_conv! {
    pub x11_button(X11: u8) {
        1 => Left,
        2 => Middle,
        3 => Right,
        // BTN_SIDE or BTN_BACK
        8 | 10 => Back,
        // BTN_EXTRA or BTN_FORWARD
        9 | 11 => Forward,
    }
}
