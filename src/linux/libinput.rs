//! Raw device events.
//!
//! This module uses [`libinput`](input) to read from `/dev/input`. Because it lacks any context
//! from a display server, it only produces keycodes[^1] and tracks its own absolute cursor position.
//!
//! [^1]: Both the `key` and `keycode` fields of [`Event::Key`] are the same.

use crate::{
    ButtonState, Event, EventError, ListenError, ScrollUnit, button::convert::linux_button,
    key::convert::linux_keycode,
};
use byor::{
    fd::{Fd, RuntimeFd},
    fs::{DirEntry, RuntimeFs},
};
use futures_core::FusedStream;
use futures_util::{FutureExt, StreamExt};
use inotify::{EventMask, Inotify, WatchMask};
use input::{
    Device, DeviceCapability, Libinput, LibinputInterface,
    event::{
        PointerEvent,
        keyboard::{KeyState, KeyboardEventTrait},
        pointer::{Axis, PointerScrollEvent},
    },
};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    os::{fd::AsFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
};
use tracing::error;

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(
        &mut self,
        path: &std::path::Path,
        flags: i32,
    ) -> Result<std::os::unix::prelude::OwnedFd, i32> {
        OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(path)
            .map(Into::into)
            .map_err(|e| e.raw_os_error().unwrap())
    }

    fn close_restricted(&mut self, _fd: std::os::unix::prelude::OwnedFd) {}
}

type Result<T, E = EventError> = std::result::Result<T, E>;

fn filter_map<Entry: DirEntry>(libinput: &mut Libinput, entry: Entry) -> Option<(PathBuf, Device)> {
    let path = entry.path();
    let device = libinput.path_add_device(&path.display().to_string())?;
    if !device.has_capability(DeviceCapability::Keyboard)
        && !device.has_capability(DeviceCapability::Pointer)
    {
        libinput.path_remove_device(device);
        None
    } else {
        Some((path, device))
    }
}

pub async fn events<Rt: RuntimeFd + RuntimeFs>()
-> Result<impl FusedStream<Item = Result<Event>>, ListenError> {
    let mut libinput = Libinput::new_from_path(Interface);
    let mut devices = Rt::read_dir("/dev/input")
        .await?
        .filter_map(async |r| {
            let e = r.ok()?;
            if e.file_type().await.ok()?.is_dir() {
                None
            } else {
                Some(e)
            }
        })
        .filter_map(|e| std::future::ready(filter_map(&mut libinput, e)))
        .collect::<HashMap<_, _>>()
        .await;

    let mut inotify = Inotify::init()?;
    inotify.watches().add(
        "/dev/input",
        WatchMask::CREATE | WatchMask::DELETE | WatchMask::ATTRIB,
    )?;
    let inotify_fd = Rt::Fd::new(inotify.as_fd().try_clone_to_owned()?)?;
    let libinput_fd = Rt::Fd::new(libinput.as_fd().try_clone_to_owned()?)?;
    let [mut cursor_x, mut cursor_y] = [0.0; _];

    Ok(async_stream::stream! {
        loop {
            futures_util::select! {
                r = inotify_fd.readable().fuse() => {
                    if let Err(error) = r {
                        error!(%error, "Failed to poll inotify fd");
                        yield Err(error.into());
                        continue;
                    }
                    let mut buf = [0; 1024];
                    loop {
                        match inotify.read_events(&mut buf) {
                            Ok(events) => handle_inotify_events(events, &mut libinput, &mut devices),
                            Err(error) => {
                                if error.kind() == std::io::ErrorKind::WouldBlock {
                                    break;
                                } else {
                                    error!(%error, "Failed to read inotify events");
                                    yield Err(error.into());
                                    continue;
                                }
                            }
                        }
                    }
                }
                r = libinput_fd.readable().fuse() => {
                    if let Err(error) = r {
                        error!(%error, "Failed to poll libinput fd");
                        yield Err(error.into());
                        continue;
                    }
                    if let Err(error) = libinput.dispatch() {
                        error!(%error, "Failed to get libinput events");
                        yield Err(error.into());
                        continue;
                    }
                    for event in get_libinput_events(&mut libinput, (&mut cursor_x, &mut cursor_y)) {
                        yield Ok(event);
                    }
                }
            }
        }
    })
}

impl From<KeyState> for ButtonState {
    fn from(value: KeyState) -> Self {
        match value {
            KeyState::Pressed => Self::Pressed,
            KeyState::Released => Self::Released,
        }
    }
}

impl From<input::event::pointer::ButtonState> for ButtonState {
    fn from(value: input::event::pointer::ButtonState) -> Self {
        use input::event::pointer::ButtonState::*;
        match value {
            Pressed => ButtonState::Pressed,
            Released => ButtonState::Released,
        }
    }
}

fn get_libinput_events(
    libinput: &mut Libinput,
    (x, y): (&mut f64, &mut f64),
) -> impl Iterator<Item = Event> {
    libinput.filter_map(|e| match e {
        input::Event::Keyboard(event) => {
            let keycode = linux_keycode(event.key() as u16);
            Some(Event::Key {
                keycode,
                key: keycode,
                state: event.key_state().into(),
            })
        }
        input::Event::Pointer(PointerEvent::Button(e)) => Some(Event::Button {
            button: linux_button(e.button() as u16),
            state: e.button_state().into(),
        }),
        input::Event::Pointer(PointerEvent::Motion(event)) => {
            *x += event.dx();
            *y += event.dy();
            Some(Event::Motion {
                x: *x as i32,
                y: *y as i32,
            })
        }
        input::Event::Pointer(PointerEvent::ScrollWheel(e)) => {
            let dx = if e.has_axis(Axis::Horizontal) {
                e.scroll_value_v120(Axis::Horizontal) / 120.0
            } else {
                0.0
            };
            let dy = if e.has_axis(Axis::Vertical) {
                e.scroll_value_v120(Axis::Vertical) / 120.0
            } else {
                0.0
            };
            Some(Event::Scroll {
                unit: ScrollUnit::Detent,
                dx,
                dy,
            })
        }
        input::Event::Pointer(PointerEvent::ScrollFinger(e)) => Some(e.into()),
        input::Event::Pointer(PointerEvent::ScrollContinuous(e)) => Some(e.into()),
        _ => None,
    })
}

fn scroll_value<T: PointerScrollEvent>(e: &T, axis: Axis) -> Option<f64> {
    if e.has_axis(axis) {
        Some(e.scroll_value(axis))
    } else {
        None
    }
}

impl<T: PointerScrollEvent> From<T> for Event {
    fn from(value: T) -> Self {
        Self::Scroll {
            unit: ScrollUnit::Pixel,
            dx: scroll_value(&value, Axis::Horizontal).unwrap_or_default(),
            dy: scroll_value(&value, Axis::Vertical).unwrap_or_default(),
        }
    }
}

fn handle_inotify_events<'a>(
    events: impl IntoIterator<Item = inotify::Event<&'a std::ffi::OsStr>>,
    libinput: &mut Libinput,
    devices: &mut HashMap<PathBuf, Device>,
) {
    for event in events {
        if let Some(name) = event.name {
            let path = PathBuf::from("/dev/input").join(name);
            if !devices.contains_key(&path)
                && (event.mask.contains(EventMask::CREATE)
                    || event.mask.contains(EventMask::ATTRIB))
                && let Some(device) = libinput.path_add_device(&path.display().to_string())
            {
                devices.insert(path, device);
            } else if event.mask.contains(EventMask::DELETE)
                && let Some(device) = devices.remove(&path)
            {
                libinput.path_remove_device(device);
            }
        }
    }
}
