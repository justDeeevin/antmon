use crate::{Event, EventError, ListenError};
use byor::{
    fd::RuntimeFd,
    fs::{DirEntry, File, RuntimeFs},
};
use futures_core::FusedStream;
use futures_util::{StreamExt, TryStreamExt};

type Result<T, E = EventError> = std::result::Result<T, E>;

pub fn listen<Rt: RuntimeFd + RuntimeFs>()
-> Result<impl FusedStream<Item = Result<Event>>, ListenError> {
    Ok(async_stream::try_stream! {
        let mut directories = Rt::read_dir("/dev/input").await?.and_then(|e| Rt::File::open(e.path()));
        yield Event::Motion {x: 0, y: 0};
    }
    .fuse())
}
