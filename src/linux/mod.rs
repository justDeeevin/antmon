use crate::{Event, EventError, ListenError};
use byor::fd::RuntimeFd;
use futures_core::FusedStream;

mod x11;

#[doc = include_str!("../doc_listen.md")]
pub fn listen<Rt: RuntimeFd>()
-> Result<impl FusedStream<Item = Result<Event, EventError>>, ListenError> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        todo!("wayland")
    } else if std::env::var("DISPLAY").is_ok() {
        x11::listen::<Rt>()
    } else {
        Err(ListenError::NoSession)
    }
}
