//! Device events with additional context from a Wayland server.
//!
//! This module uses the [`libinput`](super::libinput) stream under the hood, since Wayland provides no
//! mechanism for global input monitoring. This means it has many of the same limitations; however,
//! it gets as much additional context as possible from the compositor to refine the information it
//! produces.
//! - Keycodes are mapped to keys using the keymap at index 0
//! - The cursor position is clamped within the logical bounds of the display

use crate::{
    ButtonState, Event, EventError, Key, ListenError,
    key::{PlatformKey, convert::from_keysym},
    linux::libinput::LibinputStream,
};
use byor::{
    fd::{Fd, RuntimeFd},
    fs::RuntimeFs,
};
use derive_more::{Deref, DerefMut};
use futures_core::{FusedStream, Stream};
use futures_util::ready;
use memmap2::MmapOptions;
use pin_project::pin_project;
use std::{collections::HashMap, os::fd::OwnedFd, task::Poll};
use wayland_client::{
    ConnectError, Connection, Dispatch, DispatchError, WEnum,
    globals::{BindError, GlobalError, GlobalListContents, registry_queue_init},
    protocol::{
        wl_keyboard::{self, KeymapFormat, WlKeyboard},
        wl_output::WlOutput,
        wl_registry::WlRegistry,
        wl_seat::{self, Capability, WlSeat},
    },
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use xkbcommon::xkb::{
    self, CONTEXT_NO_FLAGS, Context, KEYMAP_COMPILE_NO_FLAGS, KEYMAP_FORMAT_TEXT_V1, KeyDirection,
    Keymap, State,
};

#[derive(Debug, thiserror::Error)]
#[error("Found an unknown keycode from a non-linux platform.")]
/// The error produced when a [`Key`] can't be converted into an linux keycode.
pub struct BadUnknown;

impl From<BadUnknown> for EventError {
    fn from(_value: BadUnknown) -> Self {
        Self::GetState
    }
}

fn into_keycode(key: Key) -> Result<u16, EventError> {
    match key {
        Key::Unknown(PlatformKey::Linux(code)) => Ok(code),
        Key::Unknown(_) => Err(EventError::GetState),

        _ => Ok(unsafe { *(&raw const key as *const u16) }),
    }
}

impl From<ConnectError> for ListenError {
    fn from(_value: ConnectError) -> Self {
        Self::Platform
    }
}

impl From<GlobalError> for ListenError {
    fn from(_value: GlobalError) -> Self {
        Self::Platform
    }
}

impl From<BindError> for ListenError {
    fn from(_value: BindError) -> Self {
        Self::Platform
    }
}

impl From<DispatchError> for ListenError {
    fn from(_value: DispatchError) -> Self {
        Self::Platform
    }
}

#[derive(Default, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    fn contains(&self, (x, y): (i32, i32)) -> bool {
        x > self.x && x - self.x < self.w && y > self.y && y - self.y < self.h
    }

    fn clamp(&self, (x, y): (i32, i32)) -> (i32, i32) {
        let max_x = (self.x + self.w - 1).max(self.x);
        let max_y = (self.y + self.h - 1).max(self.y);
        (x.clamp(self.x, max_x), y.clamp(self.y, max_y))
    }
}

#[derive(Default)]
struct WlGetter {
    keymap: Option<Keymap>,
    outputs: HashMap<u32, Rect>,
}

fn load_wayland_data() -> Result<(Keymap, Vec<Rect>), ListenError> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<WlGetter>(&conn)?;
    let qh = queue.handle();
    let _seat: WlSeat = globals.bind(&qh, 1..=7, ())?;
    let mut getter = WlGetter::default();

    queue.roundtrip(&mut getter)?;
    queue.roundtrip(&mut getter)?;

    let manager: ZxdgOutputManagerV1 = globals.bind(&qh, 1..=3, ())?;
    let registry = globals.registry();
    globals.contents().with_list(|globals| {
        for g in globals.iter().filter(|g| g.interface == "wl_output") {
            manager.get_xdg_output(
                &registry.bind(g.name, g.version.min(4), &qh, ()),
                &qh,
                g.name,
            );
        }
    });
    queue.roundtrip(&mut getter)?;
    // TODO: necessary?
    queue.roundtrip(&mut getter)?;

    Ok((
        getter.keymap.ok_or(ListenError::Platform)?,
        getter
            .outputs
            .into_values()
            .filter(|r| r != &Rect::default())
            .collect(),
    ))
}

impl Dispatch<WlRegistry, GlobalListContents> for WlGetter {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for WlGetter {
    fn event(
        _state: &mut Self,
        proxy: &WlSeat,
        event: <WlSeat as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
            && caps.contains(Capability::Keyboard)
        {
            proxy.get_keyboard(qhandle, ());
        }
    }
}

impl Dispatch<WlKeyboard, ()> for WlGetter {
    fn event(
        state: &mut Self,
        _proxy: &WlKeyboard,
        event: <WlKeyboard as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Keymap {
            format: WEnum::Value(KeymapFormat::XkbV1),
            size,
            fd,
        } = event
        {
            let Ok(map) = MmapOptions::new().len(size as usize).map_if_safe(&fd) else {
                return;
            };
            let Ok(text) = str::from_utf8(&map).map(|s| s.trim_end_matches('\0').to_string())
            else {
                return;
            };
            state.keymap = Keymap::new_from_string(
                &Context::new(CONTEXT_NO_FLAGS),
                text,
                KEYMAP_FORMAT_TEXT_V1,
                KEYMAP_COMPILE_NO_FLAGS,
            );
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for WlGetter {
    fn event(
        _state: &mut Self,
        _proxy: &ZxdgOutputManagerV1,
        _event: <ZxdgOutputManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlOutput, ()> for WlGetter {
    fn event(
        _state: &mut Self,
        _proxy: &WlOutput,
        _event: <WlOutput as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgOutputV1, u32> for WlGetter {
    fn event(
        state: &mut Self,
        _proxy: &ZxdgOutputV1,
        event: <ZxdgOutputV1 as wayland_client::Proxy>::Event,
        data: &u32,
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        let info = state.outputs.entry(*data).or_default();
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                info.x = x;
                info.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                info.w = width;
                info.h = height;
            }
            _ => {}
        }
    }
}

fn clamp(p: (i32, i32), outputs: &[Rect]) -> (i32, i32) {
    if outputs.iter().any(|r| r.contains(p)) {
        p
    } else {
        outputs
            .iter()
            .map(|r| r.clamp(p))
            .min_by(|a, b| dist2(*a, p).cmp(&dist2(*b, p)))
            .unwrap_or(p)
    }
}

fn dist2((x1, y1): (i32, i32), (x2, y2): (i32, i32)) -> i64 {
    let (dx, dy) = ((x1 - x2) as i64, (y1 - y2) as i64);
    (dx * dx) + (dy * dy)
}

impl From<ButtonState> for KeyDirection {
    fn from(value: ButtonState) -> Self {
        match value {
            ButtonState::Pressed => Self::Down,
            ButtonState::Released => Self::Up,
        }
    }
}

pub async fn events<Rt: RuntimeFd + RuntimeFs>()
-> Result<WaylandStream<Rt::Fd<OwnedFd>>, ListenError> {
    let (keymap, outputs) = load_wayland_data()?;
    let state = State::new(&keymap);

    Ok(WaylandStream {
        state,
        outputs,
        acc: (0, 0),
        prev: (0, 0),
        inner: super::libinput::events::<Rt>().await?,
    })
}

#[derive(Deref, DerefMut)]
#[pin_project]
pub struct WaylandStream<F> {
    state: State,
    outputs: Vec<Rect>,
    acc: (i32, i32),
    prev: (i32, i32),
    #[pin]
    #[deref]
    #[deref_mut]
    inner: LibinputStream<F>,
}

impl<F: Fd> Stream for WaylandStream<F> {
    type Item = <LibinputStream<F> as Stream>::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.project();
        Poll::Ready(Some(Ok(match ready!(this.inner.poll_next(cx)) {
            Some(Ok(Event::Key {
                keycode,
                state: key_state,
                ..
            })) => {
                let xkb_keycode = xkb::Keycode::from(into_keycode(keycode)? + 8);
                let key = from_keysym(this.state.key_get_one_sym(xkb_keycode));

                this.state.update_key(xkb_keycode, key_state.into());

                Event::Key {
                    keycode,
                    state: key_state,
                    key,
                }
            }
            Some(Ok(Event::Motion { x, y })) => {
                let (dx, dy) = (x - this.prev.0, y - this.prev.1);
                this.acc.0 += dx;
                this.acc.1 += dy;
                this.prev.0 = x;
                this.prev.1 = y;
                let (x, y) = clamp((this.acc.0, this.acc.1), this.outputs);
                this.acc.0 = x;
                this.acc.1 = y;
                Event::Motion { x, y }
            }
            o => return Poll::Ready(o),
        })))
    }
}

impl<F: Fd> FusedStream for WaylandStream<F> {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}
