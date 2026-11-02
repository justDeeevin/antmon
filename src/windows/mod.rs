use crate::{
    Button, ButtonState, Event, EventError, ListenError, ScrollUnit,
    key::convert::from_windows_vkcode,
};
use byor::channel::{
    mpsc::{RuntimeMpsc, UnboundedSender},
    oneshot::{RuntimeOneshot, Sender},
};
use futures_core::FusedStream;
use futures_util::StreamExt;
use std::{cell::RefCell, collections::HashSet, error::Error, sync::LazyLock};
use tracing::error;
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{
            HKL, KLF_NOTELLSHELL, LoadKeyboardLayoutW, MAPVK_VSC_TO_VK_EX, MapVirtualKeyExW,
        },
        WindowsAndMessaging::{
            CallNextHookEx, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, SetWindowsHookExW, WH_KEYBOARD_LL,
            WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
            WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
            WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1,
        },
    },
};
use windows_core::w;

/// An extension trait for byor UnboundedSenders that adapts their behavior in a dyn-compatible way.
trait DynSender {
    type Item: 'static;

    fn dyn_send(&self, message: Self::Item) -> Result<(), Box<dyn Error>>;
}

impl<T: UnboundedSender<SendError: 'static>> DynSender for T {
    type Item = T::Item;

    fn dyn_send(&self, message: Self::Item) -> Result<(), Box<dyn Error>> {
        self.send(message)
            .map_err(|e| Box::new(e) as Box<dyn Error>)
    }
}

thread_local! {
    static TX: RefCell<Box<dyn DynSender<Item = Event>>> = panic!("attempted to access thread-local sender before initialization");
    static PRESSED: RefCell<HashSet<u32>> = RefCell::new(HashSet::default());
}

static QWERTY: LazyLock<usize> = LazyLock::new(|| {
    unsafe { LoadKeyboardLayoutW(w!("00000409"), KLF_NOTELLSHELL) }
        .expect("Failed to load QWERTY layout")
        .0 as usize
});

pub async fn events<
    Rt: RuntimeMpsc<UnboundedSender<Event>: Send + 'static>
        + RuntimeOneshot<OneshotSender<Option<windows::core::Error>>: Send + 'static>,
>() -> Result<impl FusedStream<Item = Result<Event, EventError>>, ListenError> {
    let (tx_event, rx_event) = Rt::unbounded_channel();
    let (tx_initerror, rx_initerror) = Rt::channel();

    std::thread::spawn(move || {
        if let Err(e) =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(raw_keyboard_hook), None, 0) }
        {
            let _ = tx_initerror.send(Some(e));
            return;
        }
        if let Err(e) = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(raw_mouse_hook), None, 0) } {
            let _ = tx_initerror.send(Some(e));
            return;
        }
        let _ = tx_initerror.send(None);
        TX.set(Box::new(tx_event));
        windows_window::run();
    });

    if let Ok(Some(e)) = rx_initerror.await {
        Err(ListenError::IO(e.into()))
    } else {
        Ok(rx_event.map(Ok).fuse())
    }
}

unsafe extern "system" fn raw_keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        keyboard_hook(wparam.0 as u32, unsafe { &*(lparam.0 as *const _) });
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn keyboard_hook(ident: u32, event: &KBDLLHOOKSTRUCT) {
    let state = match ident {
        WM_KEYDOWN => {
            if !PRESSED.with_borrow_mut(|set| set.insert(event.vkCode)) {
                return;
            }
            ButtonState::Pressed
        }
        WM_KEYUP => {
            PRESSED.with_borrow_mut(|set| set.remove(&event.vkCode));
            ButtonState::Released
        }
        _ => {
            return;
        }
    };

    let key = from_windows_vkcode(event.vkCode as i32);
    let keycode = from_windows_vkcode(unsafe {
        MapVirtualKeyExW(
            event.scanCode,
            MAPVK_VSC_TO_VK_EX,
            Some(HKL(*QWERTY as *mut _)),
        )
    } as i32);

    TX.with_borrow(|tx| {
        if let Err(error) = tx.dyn_send(Event::Key {
            keycode,
            key,
            state,
        }) {
            error!(%error, "Failed to send key event");
        }
    });
}

unsafe extern "system" fn raw_mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        mouse_hook(wparam.0 as u32, unsafe { &*(lparam.0 as *const _) });
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn xbutton(data: u32) -> Button {
    if data == XBUTTON1 as u32 {
        Button::Back
    } else {
        Button::Forward
    }
}

fn scroll_delta(data: u32) -> f64 {
    let bytes = data.to_be_bytes();
    i16::from_be_bytes([bytes[0], bytes[1]]) as f64 / 120.0
}

fn mouse_hook(ident: u32, event: &MSLLHOOKSTRUCT) {
    TX.with_borrow(|tx| {
        if let Err(error) = tx.dyn_send(match ident {
            WM_LBUTTONDOWN => Event::Button {
                button: Button::Left,
                state: ButtonState::Pressed,
            },
            WM_LBUTTONUP => Event::Button {
                button: Button::Left,
                state: ButtonState::Released,
            },
            WM_RBUTTONDOWN => Event::Button {
                button: Button::Right,
                state: ButtonState::Pressed,
            },
            WM_RBUTTONUP => Event::Button {
                button: Button::Right,
                state: ButtonState::Released,
            },
            WM_MBUTTONDOWN => Event::Button {
                button: Button::Middle,
                state: ButtonState::Pressed,
            },
            WM_MBUTTONUP => Event::Button {
                button: Button::Middle,
                state: ButtonState::Released,
            },
            WM_XBUTTONDOWN => Event::Button {
                button: xbutton(event.mouseData),
                state: ButtonState::Pressed,
            },
            WM_XBUTTONUP => Event::Button {
                button: xbutton(event.mouseData),
                state: ButtonState::Released,
            },
            WM_MOUSEMOVE => Event::Motion {
                x: event.pt.x,
                y: event.pt.y,
            },
            WM_MOUSEWHEEL => Event::Scroll {
                unit: ScrollUnit::Detent,
                dx: 0.0,
                dy: -scroll_delta(event.mouseData),
            },
            WM_MOUSEHWHEEL => Event::Scroll {
                unit: ScrollUnit::Detent,
                dx: scroll_delta(event.mouseData),
                dy: 0.0,
            },
            // TODO: maybe at some point use raw input to handle extra side buttons? idfk
            _ => return,
        }) {
            error!(%error, "Failed to send mouse event");
        }
    });
}
