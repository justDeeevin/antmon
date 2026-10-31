//! Raw device events.
//!
//! This module uses [`libinput`](input) to read from `/dev/input`. Because it lacks any context
//! from a display server, it only produces keycodes[^1] and tracks its own absolute cursor position.
//!
//! Touchpad input is configured to use the [`Clickfinger`](ClickMethod::Clickfinger) click method
//! and the [`TwoFinger`](ScrollMethod::TwoFinger) scroll method by default.
//!
//! Unfortunately, in order to implement the [`LibinputInterface`] trait, opening new device files is done
//! synchronously. However, this only happens en masse on initialization and only happens
//! infrequently during the stream's lifetime.
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
use futures_core::{FusedStream, Stream};
use futures_util::{StreamExt, ready};
use inotify::{EventMask, Inotify, WatchMask};
use input::{
    ClickMethod, Device, DeviceCapability, Libinput, LibinputInterface, ScrollMethod,
    event::{
        PointerEvent,
        keyboard::{KeyState, KeyboardEventTrait},
        pointer::{Axis, PointerScrollEvent},
    },
};
use pin_project::pin_project;
use std::{
    collections::{HashMap, VecDeque},
    fs::OpenOptions,
    io::ErrorKind,
    os::{
        fd::{AsFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
    task::Poll,
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

fn open_device<Entry: DirEntry>(
    libinput: &mut Libinput,
    entry: Entry,
) -> Option<(PathBuf, Device)> {
    let path = entry.path();
    let mut device = libinput.path_add_device(&path.display().to_string())?;
    if !device.has_capability(DeviceCapability::Keyboard)
        && !device.has_capability(DeviceCapability::Pointer)
    {
        libinput.path_remove_device(device);
        None
    } else {
        let _ = device.config_scroll_set_method(ScrollMethod::TwoFinger);
        let _ = device.config_click_set_method(ClickMethod::Clickfinger);
        Some((path, device))
    }
}

pub async fn events<Rt: RuntimeFd + RuntimeFs>()
-> Result<LibinputStream<Rt::Fd<OwnedFd>>, ListenError> {
    let mut libinput = Libinput::new_from_path(Interface);
    let devices = Rt::read_dir("/dev/input")
        .await?
        .filter_map(async |r| {
            let e = r.ok()?;
            if e.file_type().await.ok()?.is_dir() {
                None
            } else {
                Some(e)
            }
        })
        .filter_map(|e| std::future::ready(open_device(&mut libinput, e)))
        .collect::<HashMap<_, _>>()
        .await;

    let inotify = Inotify::init()?;
    inotify.watches().add(
        "/dev/input",
        WatchMask::CREATE | WatchMask::DELETE | WatchMask::ATTRIB,
    )?;
    let inotify_fd = Rt::Fd::new(inotify.as_fd().try_clone_to_owned()?)?;
    let libinput_fd = Rt::Fd::new(libinput.as_fd().try_clone_to_owned()?)?;

    Ok(LibinputStream {
        devices,
        inotify_fd,
        inotify,
        libinput_fd,
        libinput,
        cursor: (0.0, 0.0),
        queued_events: VecDeque::default(),
    })
}

#[pin_project]
pub struct LibinputStream<F> {
    devices: HashMap<PathBuf, Device>,
    #[pin]
    inotify_fd: F,
    inotify: Inotify,
    #[pin]
    libinput_fd: F,
    libinput: Libinput,
    cursor: (f64, f64),
    queued_events: VecDeque<Event>,
}

impl<F: Fd> Stream for LibinputStream<F> {
    type Item = Result<Event>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.project();

        let mut out = this.queued_events.pop_front();

        loop {
            match this.inotify_fd.poll_readable(cx) {
                Poll::Ready(Ok(())) => {
                    let mut buf = [0; 1024];
                    loop {
                        match this.inotify.read_events(&mut buf) {
                            Ok(events) => {
                                for event in events {
                                    if let Some(name) = event.name {
                                        let path = PathBuf::from("/dev/input").join(name);
                                        if !this.devices.contains_key(&path)
                                            && (event.mask.contains(EventMask::CREATE)
                                                || event.mask.contains(EventMask::ATTRIB))
                                            && let Some(device) = this
                                                .libinput
                                                .path_add_device(&path.display().to_string())
                                        {
                                            this.devices.insert(path, device);
                                        } else if event.mask.contains(EventMask::DELETE)
                                            && let Some(device) = this.devices.remove(&path)
                                        {
                                            this.libinput.path_remove_device(device);
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                if error.kind() == ErrorKind::WouldBlock {
                                    break;
                                } else {
                                    error!(%error, "Failed to read inotify events");
                                    return Poll::Ready(Some(Err(error.into())));
                                }
                            }
                        }
                    }
                }
                Poll::Ready(Err(error)) => {
                    error!(%error, "Failed to poll inotify fd");
                    return Poll::Ready(Some(Err(error.into())));
                }
                Poll::Pending => {}
            }

            if let Err(error) = ready!(this.libinput_fd.poll_readable(cx)) {
                error!(%error, "Failed to poll libinput fd");
                return Poll::Ready(Some(Err(error.into())));
            }

            if let Err(error) = this.libinput.dispatch() {
                error!(%error, "Failed to get libinput events");
                return Poll::Ready(Some(Err(error.into())));
            }

            let mut events = this.libinput.filter_map(|e| match e {
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
                    this.cursor.0 += event.dx();
                    this.cursor.1 += event.dy();
                    Some(Event::Motion {
                        x: this.cursor.0 as i32,
                        y: this.cursor.1 as i32,
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
            });

            if out.is_none() {
                out = events.next();
            }

            this.queued_events.extend(events);

            if out.is_some() {
                return Poll::Ready(out.map(Ok));
            }
        }
    }
}

impl<F: Fd> FusedStream for LibinputStream<F> {
    fn is_terminated(&self) -> bool {
        false
    }
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
