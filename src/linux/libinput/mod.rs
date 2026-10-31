//! Raw device events.
//!
//! This module uses [`libinput`](input) to read from `/dev/input`. Because it lacks any context
//! from a display server, it only produces keycodes[^1] and tracks its own absolute cursor position.
//!
//! Touchpad input is configured to use the [`Clickfinger`](ClickMethod::Clickfinger) click method
//! and the [`TwoFinger`](ScrollMethod::TwoFinger) scroll method by default. You can use
//! [`devices_mut`](LibinputStream::devices_mut) or
//! [`devices_list_mut`](LibinputStream::devices_list_mut) to change this configuration per-device.
//! There are also methods to set default values for each config key that will be automatically
//! applied to all existing devices and any newly-connected ones.
//!
//! Unfortunately, in order to implement the [`LibinputInterface`] trait, opening new device files is done
//! synchronously. However, this only happens en masse on initialization and only happens
//! infrequently during the stream's lifetime.
//!
//! [^1]: Both the `key` and `keycode` fields of [`Event::Key`] are the same.

use crate::{
    ButtonState, Event, EventError, ListenError, ScrollUnit, button::convert::linux_button,
    key::convert::from_linux_keycode, linux::libinput::config::Config,
};
use byor::{
    fd::{Fd, RuntimeFd},
    fs::{DirEntry, RuntimeFs},
};
use futures_core::{FusedStream, Stream};
use futures_util::{StreamExt, ready};
use inotify::{EventMask, Inotify, WatchMask};
use input::{
    event::{
        PointerEvent,
        keyboard::{KeyState, KeyboardEventTrait},
        pointer::{Axis, PointerScrollEvent},
    },
    *,
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

mod config;

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &std::path::Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(path)
            .map(Into::into)
            .map_err(|e| e.raw_os_error().unwrap())
    }

    fn close_restricted(&mut self, _fd: OwnedFd) {}
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
        defaults: Config::default(),
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
    defaults: Config,
}

impl<F> LibinputStream<F> {
    /// Get all of the devices that are currently plugged in.
    pub fn devices(&self) -> &HashMap<PathBuf, Device> {
        &self.devices
    }

    /// Get a list of all of the devices that are currently plugged in.
    pub fn devices_list(&self) -> impl Iterator<Item = &Device> {
        self.devices.values()
    }

    /// Get mutable access to all of the devices that are currently plugged in.
    ///
    /// This can be used to configure devices as desired.
    ///
    /// Note that this allocates a new map (though the keys and values are references to those owned
    /// by the stream). If you don't need to look up devices by path, use
    /// [`devices_list_mut`](Self::devices_list_mut) to save resources.
    pub fn devices_mut(&mut self) -> HashMap<&Path, &mut Device> {
        self.devices
            .iter_mut()
            .map(|(p, d)| (p.as_ref(), d))
            .collect()
    }

    /// Get a list of mutable references to all of the devices that are
    /// currently plugged in.
    ///
    /// This can be used to configure devices as desired.
    pub fn devices_list_mut(&mut self) -> impl Iterator<Item = &mut Device> {
        self.devices.values_mut()
    }

    pub fn set_default_accel_config(&mut self, constructor: Option<fn() -> AccelConfig>) {
        if constructor.map(|c| c()) == self.defaults.accel_config.map(|c| c()) {
            return;
        }
        self.defaults.accel_config = constructor;
        self.defaults.profile_override = None;
        if let Some(constructor) = constructor {
            for device in self.devices.values_mut() {
                let _ = device.config_accel_apply(constructor());
            }
        }
    }

    pub fn set_default_accel_profile(&mut self, profile: Option<AccelProfile>) {
        if profile == self.defaults.profile_override {
            return;
        }
        self.defaults.profile_override = profile;
        for device in self.devices_list_mut() {
            if let Some(profile) = profile.or_else(|| device.config_accel_default_profile()) {
                let _ = device.config_accel_set_profile(profile);
            }
        }
    }

    pub fn set_default_accel_speed(&mut self, speed: Option<f64>) {
        if speed == self.defaults.accel_speed {
            return;
        }
        self.defaults.accel_speed = speed;
        for device in self.devices_list_mut() {
            let _ = device.config_accel_set_speed(
                speed.unwrap_or_else(|| device.config_accel_default_speed()),
            );
        }
    }

    pub fn set_default_area_rectangle(&mut self, rect: Option<AreaRectangle>) {
        if rect == self.defaults.area_rectangle {
            return;
        }
        self.defaults.area_rectangle = rect;
        for device in self.devices_list_mut() {
            let _ = device.config_area_set_rectangle(
                rect.unwrap_or_else(|| device.config_area_get_default_rectangle()),
            );
        }
    }

    pub fn set_default_calibration_matrix(&mut self, matrix: Option<[f32; 6]>) {
        if matrix == self.defaults.calibration_matrix {
            return;
        }
        self.defaults.calibration_matrix = matrix;
        for device in self.devices_list_mut() {
            if let Some(matrix) = matrix.or_else(|| device.config_calibration_default_matrix()) {
                let _ = device.config_calibration_set_matrix(matrix);
            }
        }
    }

    pub fn set_default_click_method(&mut self, method: Option<ClickMethod>) {
        if method == self.defaults.click_method {
            return;
        }
        self.defaults.click_method = method;
        for device in self.devices_list_mut() {
            if let Some(method) = method.or_else(|| device.config_click_default_method()) {
                let _ = device.config_click_set_method(method);
            }
        }
    }

    pub fn set_default_clickfinger_button_map(&mut self, map: Option<ClickfingerButtonMap>) {
        if map == self.defaults.clickfinger_button_map {
            return;
        }
        self.defaults.clickfinger_button_map = map;
        for device in self.devices_list_mut() {
            let _ = device.config_click_clickfinger_set_button_map(
                map.unwrap_or_else(|| device.config_click_clickfinger_default_button_map()),
            );
        }
    }

    pub fn set_default_dwt_enabled(&mut self, enabled: Option<bool>) {
        if enabled == self.defaults.dwt_enabled {
            return;
        }
        self.defaults.dwt_enabled = enabled;
        for device in self.devices_list_mut() {
            let _ = device.config_dwt_set_enabled(
                enabled.unwrap_or_else(|| device.config_dwt_default_enabled()),
            );
        }
    }

    pub fn set_default_dwtp_enabled(&mut self, enabled: Option<bool>) {
        if enabled == self.defaults.dwtp_enabled {
            return;
        }
        self.defaults.dwtp_enabled = enabled;
        for device in self.devices_list_mut() {
            let _ = device.config_dwtp_set_enabled(
                enabled.unwrap_or_else(|| device.config_dwtp_default_enabled()),
            );
        }
    }

    pub fn set_default_middle_emulation_enabled(&mut self, enabled: Option<bool>) {
        if enabled == self.defaults.middle_emulation_enabled {
            return;
        }
        self.defaults.middle_emulation_enabled = enabled;
        for device in self.devices_list_mut() {
            let _ = device.config_middle_emulation_set_enabled(
                enabled.unwrap_or_else(|| device.config_middle_emulation_default_enabled()),
            );
        }
    }

    pub fn set_default_rotation_angle(&mut self, angle: Option<u32>) {
        if angle == self.defaults.rotation_angle {
            return;
        }
        self.defaults.rotation_angle = angle;
        for device in self.devices_list_mut() {
            let _ = device.config_rotation_set_angle(
                angle.unwrap_or_else(|| device.config_rotation_default_angle()),
            );
        }
    }

    pub fn set_default_scroll_method(&mut self, method: Option<ScrollMethod>) {
        if method == self.defaults.scroll_method {
            return;
        }
        self.defaults.scroll_method = method;
        for device in self.devices_list_mut() {
            if let Some(method) = method.or_else(|| device.config_scroll_default_method()) {
                let _ = device.config_scroll_set_method(method);
            }
        }
    }

    pub fn set_default_natural_scroll_enabled(&mut self, enabled: Option<bool>) {
        if enabled == self.defaults.natural_scroll_enabled {
            return;
        }
        self.defaults.natural_scroll_enabled = enabled;
        for device in self.devices_list_mut() {
            let _ = device.config_scroll_set_natural_scroll_enabled(
                enabled.unwrap_or_else(|| device.config_scroll_default_natural_scroll_enabled()),
            );
        }
    }

    pub fn set_default_scroll_button(&mut self, button: Option<u32>) {
        if button == self.defaults.scroll_button {
            return;
        }
        self.defaults.scroll_button = button;
        for device in self.devices_list_mut() {
            let _ = device.config_scroll_set_button(
                button.unwrap_or_else(|| device.config_scroll_default_button()),
            );
        }
    }

    pub fn set_default_scroll_button_lock(&mut self, state: Option<ScrollButtonLockState>) {
        if state == self.defaults.scroll_button_lock {
            return;
        }
        self.defaults.scroll_button_lock = state;
        for device in self.devices_list_mut() {
            let _ = device.config_scroll_set_button_lock(
                state.unwrap_or_else(|| device.config_scroll_default_button_lock()),
            );
        }
    }

    pub fn set_default_send_events_mode(&mut self, mode: SendEventsMode) {
        if self.defaults.send_events_mode == Some(mode) {
            return;
        }
        self.defaults.send_events_mode = Some(mode);
        for device in self.devices_list_mut() {
            let _ = device.config_send_events_set_mode(mode);
        }
    }

    pub fn set_default_3fg_drag_enabled(&mut self, state: Option<ThreeFingerDragState>) {
        if state == self.defaults.threefg_drag_enabled {
            return;
        }
        self.defaults.threefg_drag_enabled = state;
        for device in self.devices_list_mut() {
            let _ = device.config_3fg_drag_set_enabled(
                state.unwrap_or_else(|| device.config_3fg_drag_get_default_enabled()),
            );
        }
    }

    pub fn set_default_tap_button_map(&mut self, map: Option<TapButtonMap>) {
        if map == self.defaults.tap_button_map {
            return;
        }
        self.defaults.tap_button_map = map;
        for device in self.devices_list_mut() {
            if let Some(map) = map.or_else(|| device.config_tap_default_button_map()) {
                let _ = device.config_tap_set_button_map(map);
            }
        }
    }

    pub fn set_default_tap_drag_enabled(&mut self, enabled: Option<bool>) {
        if enabled == self.defaults.tap_drag_enabled {
            return;
        }
        self.defaults.tap_drag_enabled = enabled;
        for device in self.devices_list_mut() {
            let _ = device.config_tap_set_drag_enabled(
                enabled.unwrap_or_else(|| device.config_tap_default_drag_enabled()),
            );
        }
    }

    pub fn set_default_tap_drag_lock_enabled(&mut self, state: Option<DragLockState>) {
        if state == self.defaults.tap_drag_lock_enabled {
            return;
        }
        self.defaults.tap_drag_lock_enabled = state;
        for device in self.devices_list_mut() {
            let _ = device.config_tap_set_drag_lock_enabled(
                state.unwrap_or_else(|| device.config_tap_default_drag_lock_enabled()),
            );
        }
    }
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
                                        let path_string = format!("/dev/input/{}", name.display());
                                        let path = PathBuf::from(&path_string);
                                        if !this.devices.contains_key(&path)
                                            && (event.mask.contains(EventMask::CREATE)
                                                || event.mask.contains(EventMask::ATTRIB))
                                            && let Some(mut device) =
                                                this.libinput.path_add_device(&path_string)
                                        {
                                            this.defaults.apply(&mut device);
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
                    let keycode = from_linux_keycode(event.key() as u16);
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
