use crate::{
    Button, ButtonState, Event, EventError, Key, ListenError, ScrollUnit,
    button::convert::from_macos_button,
    key::convert::{from_char, from_macos_keycode},
};
use byor::channel::{
    mpsc::{RuntimeMpsc, Sender as _, UnboundedSender},
    oneshot::{RuntimeOneshot, Sender as _},
};
use core_foundation::{
    runloop::{CFRunLoop, kCFRunLoopDefaultMode},
    string::UniChar,
};
use core_graphics::{
    event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        CallbackResult, EventField,
    },
    sys::{CGEvent, CGEventRef},
};
use foreign_types::ForeignType;
use futures_core::FusedStream;
use futures_util::StreamExt;
use tracing::error;

pub async fn events<
    Rt: RuntimeOneshot<OneshotSender<Option<()>>: Send + 'static>
        + RuntimeMpsc<UnboundedSender<Event>: Send + 'static>,
>() -> Result<impl FusedStream<Item = Result<Event, EventError>>, ListenError> {
    let (tx_event, rx_event) = Rt::unbounded_channel();
    let (tx_initerror, rx_initerror) = Rt::channel();

    std::thread::spawn(move || {
        let Ok(tap) = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
                CGEventType::MouseMoved,
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::ScrollWheel,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
            ],
            move |_proxy, kind, event| {
                let event = match kind {
                    CGEventType::LeftMouseDown => Event::Button {
                        button: Button::Left,
                        state: ButtonState::Pressed,
                    },
                    CGEventType::LeftMouseUp => Event::Button {
                        button: Button::Left,
                        state: ButtonState::Released,
                    },
                    CGEventType::RightMouseDown => Event::Button {
                        button: Button::Right,
                        state: ButtonState::Pressed,
                    },
                    CGEventType::RightMouseUp => Event::Button {
                        button: Button::Right,
                        state: ButtonState::Released,
                    },
                    CGEventType::MouseMoved => {
                        let location = event.location();
                        Event::Motion {
                            x: location.x.floor() as i32,
                            y: location.y.floor() as i32,
                        }
                    }
                    CGEventType::KeyDown => {
                        if event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0
                        {
                            return CallbackResult::Keep;
                        }
                        let keycode = from_macos_keycode(
                            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
                                as u16,
                        );

                        Event::Key {
                            key: get_key(keycode, event),
                            keycode,
                            state: ButtonState::Pressed,
                        }
                    }
                    CGEventType::KeyUp => {
                        let keycode = from_macos_keycode(
                            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
                                as u16,
                        );
                        Event::Key {
                            key: get_key(keycode, event),
                            keycode,
                            state: ButtonState::Released,
                        }
                    }
                    CGEventType::ScrollWheel => Event::Scroll {
                        unit: ScrollUnit::Detent,
                        dx: event.get_double_value_field(
                            EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
                        ),
                        dy: event.get_double_value_field(
                            EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
                        ),
                    },
                    CGEventType::OtherMouseDown => Event::Button {
                        button: from_macos_button(
                            event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER),
                        ),
                        state: ButtonState::Pressed,
                    },
                    CGEventType::OtherMouseUp => Event::Button {
                        button: from_macos_button(
                            event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER),
                        ),
                        state: ButtonState::Released,
                    },
                    _ => return CallbackResult::Keep,
                };

                if let Err(error) = tx_event.send(event) {
                    if tx_event.is_closed() {
                        CFRunLoop::get_current().stop();
                    } else {
                        error!(%error, "Failed to send event");
                    }
                }

                CallbackResult::Keep
            },
        ) else {
            let _ = tx_initerror.send(Some(()));
            return;
        };

        let Ok(source) = tap.mach_port().create_runloop_source(0) else {
            let _ = tx_initerror.send(Some(()));
            return;
        };
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopDefaultMode });
        tap.enable();
        let _ = tx_initerror.send(None);
        CFRunLoop::run_current();
    });

    let init_res = rx_initerror.await;

    if init_res.is_err() || matches!(init_res, Ok(Some(()))) {
        Err(ListenError::Platform)
    } else {
        Ok(rx_event.map(Ok).fuse())
    }
}

fn get_key(keycode: Key, event: &impl ForeignType<CType = CGEvent>) -> Key {
    let mut buf = [0; 2];
    let mut actual_len = 0;

    unsafe {
        CGEventKeyboardGetUnicodeString(
            event.as_ptr(),
            buf.len() as UniCharCount,
            &raw mut actual_len,
            buf.as_mut_ptr(),
        );
    }
    if keycode.is_char()
        && let Some(key) =
            char::decode_utf16(buf[..(actual_len as usize).min(buf.len())].iter().copied())
                .next()
                .and_then(Result::ok)
                .and_then(from_char)
    {
        key
    } else {
        keycode
    }
}

type UniCharCount = std::ffi::c_ulong;

#[cfg_attr(
    all(target_arch = "aarch64", target_os = "macos"),
    link(name = "CoreGraphics", kind = "framework")
)]
unsafe extern "C" {
    fn CGEventKeyboardGetUnicodeString(
        event: CGEventRef,
        maxStringLength: UniCharCount,
        actualStringLength: *mut UniCharCount,
        unicodeString: *mut UniChar,
    );
}
