//! Raw device events.
//!
//! This module uses [`evdev`] to read from `/dev/input`. Because it lacks any context from a
//! display server, it only produces keycodes[^1] and relative mouse motion[^2].
//!
//! In addition to only producing mouse deltas, evdev mouse events are only provided in a single axis at a
//! time. In order to reduce noise, antmon's stream groups each pair of events. This is based on the
//! assumption that every motion event moves in both axes by some amount. While this is not always
//! true, it is very rarely false, and when it is, the error is very small.
//!
//! [^1]: Both the `key` and `keycode` fields of [`Event::Key`] are the same.
//! [^2]: The `x` and `y` fields of [`Event::Motion`] represent how far the cursor moved rather than
//! where it is on the screen.

use crate::{
    ButtonState, Event, EventError, ListenError, button::convert::linux_button,
    key::convert::linux_keycode,
};
use byor::{
    fd::{Fd, RuntimeFd},
    fs::{DirEntry, File, RuntimeFs},
};
use evdev::{Device, EventSummary, EventType, InputEvent, KeyCode, RelativeAxisCode};
use futures_core::FusedStream;
use futures_util::{FutureExt, StreamExt, TryFutureExt, stream::FuturesUnordered};
use inotify::{EventMask, Inotify, WatchMask};
use std::{
    collections::HashMap,
    os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
    path::PathBuf,
};
use tracing::error;

type Result<T, E = EventError> = std::result::Result<T, E>;

struct DeviceExt<F> {
    device: Device,
    fd: F,
    hires: bool,
    cursor: Cursor,
}

#[derive(Default)]
struct Cursor {
    x: i32,
    y: i32,
    send: bool,
}

impl Cursor {
    fn delta(&mut self, x: i32, y: i32) -> Option<(i32, i32)> {
        self.x += x;
        self.y += y;
        self.send = !self.send;
        if !self.send {
            Some((self.x, self.y))
        } else {
            None
        }
    }
}

pub async fn events<Rt: RuntimeFd + RuntimeFs>()
-> Result<impl FusedStream<Item = Result<Event>>, ListenError> {
    let mut inotify = Inotify::init()?;
    inotify.watches().add(
        "/dev/input",
        WatchMask::CREATE | WatchMask::DELETE | WatchMask::ATTRIB,
    )?;
    let inotify_fd = Rt::Fd::new(inotify.as_fd().try_clone_to_owned()?)?;

    let mut devices = Rt::read_dir("/dev/input")
        .await?
        .filter_map(async |e| {
            let path = e.ok()?.path();
            let info = open_device::<Rt>(&path).await.ok()?;
            let events = info.device.supported_events();
            if events.contains(EventType::KEY) || events.contains(EventType::RELATIVE) {
                Some((path, info))
            } else {
                None
            }
        })
        .collect::<HashMap<_, _>>()
        .await;

    Ok(async_stream::stream! {
        loop {
            futures_util::select! {
                r = inotify_fd.readable().fuse() => {
                    if let Err(e) = r {
                        error!(%e, "Failed to poll inotify fd");
                        yield Err(e.into());
                        continue;
                    }
                    let mut buf = [0; 1024];
                    loop {
                        match inotify.read_events(&mut buf) {
                            Ok(events) => handle_inotify_events::<Rt>(events, &mut devices).await,
                            Err(e) => {
                                if e.kind() == std::io::ErrorKind::WouldBlock {
                                    break;
                                } else {
                                    error!(%e, "Failed to read inotify events");
                                    yield Err(e.into());
                                    continue;
                                }
                            }
                        };
                    }
                }
                r = next_ready::<Rt>(&devices).fuse() => {
                        let path = match r {
                            Ok(path) => path,
                            Err(e) => {
                                error!(%e, "Failed to poll device fd");
                                yield Err(e);
                                continue;
                            }
                        };
                        let info = devices.get_mut(&path).unwrap();
                        loop {
                            let events = match info.device.fetch_events() {
                                Ok(events) => events,
                                Err(e) => {
                                    error!(%e, "Failed to fetch evdev events");
                                    break;
                                }
                            };
                            for event in events {
                                if let Some(e) = make_event(event, &mut info.hires, &mut info.cursor) {
                                    yield Ok(e);
                                }
                            }
                        }
                    }
            };
        }
    }
    .fuse())
}

async fn handle_inotify_events<Rt: RuntimeFs + RuntimeFd>(
    events: impl IntoIterator<Item = inotify::Event<&std::ffi::OsStr>>,
    devices: &mut HashMap<PathBuf, DeviceExt<Rt::Fd<OwnedFd>>>,
) {
    for event in events {
        if let Some(name) = event.name {
            let path = PathBuf::from("/dev/input").join(name);
            if !devices.contains_key(&path)
                && (event.mask.contains(EventMask::CREATE)
                    || event.mask.contains(EventMask::ATTRIB))
                && let Ok(info) = open_device::<Rt>(&path).await
            {
                let events = info.device.supported_events();
                if events.contains(EventType::KEY) || events.contains(EventType::RELATIVE) {
                    devices.insert(path, info);
                }
            } else if event.mask.contains(EventMask::DELETE) {
                devices.remove(&path);
            }
        }
    }
}

fn make_event(e: InputEvent, hires: &mut bool, cursor: &mut Cursor) -> Option<Event> {
    Some(match e.destructure() {
        EventSummary::Key(_, keycode, state) => {
            let state = if state == 2 {
                return None;
            } else if state == 1 {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };
            macro_rules! literal_keycode_const {
                ($name:ident) => {
                    const $name: u16 = KeyCode::$name.0;
                };
            }
            literal_keycode_const!(BTN_0);
            literal_keycode_const!(KEY_OK);
            literal_keycode_const!(BTN_TRIGGER_HAPPY1);
            literal_keycode_const!(BTN_TRIGGER_HAPPY40);
            match keycode.0 {
                button @ (BTN_0..KEY_OK | BTN_TRIGGER_HAPPY1..=BTN_TRIGGER_HAPPY40) => {
                    Event::Button {
                        button: linux_button(button),
                        state,
                    }
                }
                keycode => {
                    let keycode = linux_keycode(keycode);
                    Event::Key {
                        keycode,
                        key: keycode,
                        state,
                    }
                }
            }
        }
        EventSummary::RelativeAxis(_, code, value) => match code {
            RelativeAxisCode::REL_X => cursor
                .delta(value, 0)
                .map(|(x, y)| Event::Motion { x, y })?,
            RelativeAxisCode::REL_Y => cursor
                .delta(0, value)
                .map(|(x, y)| Event::Motion { x, y })?,
            RelativeAxisCode::REL_WHEEL_HI_RES => {
                if !*hires {
                    *hires = true;
                }
                Event::Scroll {
                    dx: 0.0,
                    dy: -value as f64 / 120.0,
                }
            }
            RelativeAxisCode::REL_WHEEL => {
                if *hires {
                    return None;
                } else {
                    Event::Scroll {
                        dx: 0.0,
                        dy: -value as f64,
                    }
                }
            }
            RelativeAxisCode::REL_HWHEEL_HI_RES => {
                if !*hires {
                    *hires = true;
                }
                Event::Scroll {
                    dx: value as f64 / 120.0,
                    dy: 0.0,
                }
            }
            RelativeAxisCode::REL_HWHEEL => {
                if *hires {
                    return None;
                } else {
                    Event::Scroll {
                        dx: value as f64,
                        dy: 0.0,
                    }
                }
            }
            _ => return None,
        },
        _ => return None,
    })
}

async fn open_device<Rt: RuntimeFs + RuntimeFd>(
    path: impl AsRef<std::path::Path>,
) -> std::io::Result<DeviceExt<Rt::Fd<OwnedFd>>> {
    let file = Rt::File::open(path).await?;
    let device =
        Device::from_fd(unsafe { BorrowedFd::borrow_raw(file.as_raw_fd()) }.try_clone_to_owned()?)?;
    device.set_nonblocking(true)?;
    let fd = Rt::Fd::new(device.as_fd().try_clone_to_owned()?)?;
    Ok(DeviceExt {
        device,
        fd,
        hires: false,
        cursor: Cursor::default(),
    })
}

async fn next_ready<Rt: RuntimeFd>(
    devices: &HashMap<PathBuf, DeviceExt<Rt::Fd<OwnedFd>>>,
) -> Result<PathBuf> {
    let Some(ready) = devices
        .iter()
        .map(|(path, DeviceExt { fd, .. })| fd.readable().map_ok(|()| path.clone()))
        .collect::<FuturesUnordered<_>>()
        .next()
        .await
        .transpose()?
    else {
        return std::future::pending().await;
    };
    Ok(ready)
}
