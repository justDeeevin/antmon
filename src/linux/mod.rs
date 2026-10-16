use crate::{Event, EventError, ListenError};
use byor::{fd::RuntimeFd, fs::RuntimeFs};
use futures_core::FusedStream;
use std::pin::Pin;

mod evdev;
mod x11;

#[doc = include_str!("../doc_listen.md")]
pub async fn listen<Rt: RuntimeFd + RuntimeFs + 'static>()
-> Result<impl FusedStream<Item = Result<Event, EventError>>, ListenError> {
    type BoxedFuseStream<T> = Pin<Box<dyn FusedStream<Item = T>>>;

    match std::env::var("XDG_SESSION_TYPE").as_deref() {
        Ok("x11") => x11::listen::<Rt>().map(|s| Box::pin(s) as BoxedFuseStream<_>),
        Ok("wayland") => todo!("wayland"),
        _ => evdev::listen::<Rt>()
            .await
            .map(|s| Box::pin(s) as BoxedFuseStream<_>),
    }
}
