use crate::{Event, EventError, ListenError};
use byor::{fd::RuntimeFd, fs::RuntimeFs};
use futures_core::FusedStream;
use std::pin::Pin;

mod wayland;
mod x11;

#[doc = include_str!("../doc_listen.md")]
pub fn listen<Rt: RuntimeFd + RuntimeFs + 'static>()
-> Result<impl FusedStream<Item = Result<Event, EventError>>, ListenError> {
    type BoxedFuseStream<T> = Pin<Box<dyn FusedStream<Item = T>>>;

    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        wayland::listen::<Rt>().map(|s| Box::pin(s) as BoxedFuseStream<_>)
    } else if std::env::var("DISPLAY").is_ok() {
        x11::listen::<Rt>().map(|s| Box::pin(s) as BoxedFuseStream<_>)
    } else {
        Err(ListenError::NoSession)
    }
}
