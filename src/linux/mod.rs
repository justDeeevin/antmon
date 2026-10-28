//! Linux behavior.
//!
//! Antmon will work on linux regardless of the display server used (and even if there is no
//! graphical session). However, there are caveats to non-X11 environments:
//! 1. In a TTY, antmon will only be able to provide raw keycodes and internally-tracked cursor
//!    position (see [`libinput`]). Smooth scrolling is tracked without issue.
//! 2. In Wayland, XWayland windows will be ignored. If you wish to listen to both
//!    Wayland and X11 inputs, you will need to produce a separate platform-specific stream for each.
//! 3. In both Wayland and a TTY, the program needs to be run by either root or a user in the
//!    `input` group in order to be able to read from `/dev/input`. Note that it is generally
//!    ill-advised to place a regular user in `input`. Because of these problems, the libinput solution
//!    is somewhat of a hack, but it's the only way to realistically work under Wayland.
//! 4. While Wayland is better than libinput, it is limited to the first configured keymap, and
//!    the cursor position is still dead-reckoned (albeit clamped within the display boundaries).

use crate::{Event, EventError, ListenError};
use byor::{fd::RuntimeFd, fs::RuntimeFs};
use futures_core::FusedStream;
use std::pin::Pin;

pub mod libinput;
pub mod wayland;
pub mod x11;

pub async fn events<Rt: RuntimeFd + RuntimeFs + 'static>()
-> Result<impl FusedStream<Item = Result<Event, EventError>>, ListenError> {
    type BoxedFuseStream<T> = Pin<Box<dyn FusedStream<Item = T>>>;

    match std::env::var("XDG_SESSION_TYPE").as_deref() {
        Ok("x11") => x11::events::<Rt>().map(|s| Box::pin(s) as BoxedFuseStream<_>),
        Ok("wayland") => wayland::events::<Rt>()
            .await
            .map(|s| Box::pin(s) as BoxedFuseStream<_>),
        _ => libinput::events::<Rt>()
            .await
            .map(|s| Box::pin(s) as BoxedFuseStream<_>),
    }
}
