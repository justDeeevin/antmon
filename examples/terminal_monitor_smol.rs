mod common;

use futures_util::{StreamExt, pin_mut};

fn main() {
    let mut timer = smol::Timer::interval(common::TICK_DURATION).fuse();
    smol::block_on(async {
        let stream = antmon::events::<byor::runtime::Smol>().await.unwrap();
        pin_mut!(stream);
        loop {
            futures_util::select! {
                _ = timer.next() => println!("tick"),
                r = stream.next() => println!("{:?}", r.unwrap()),
            }
        }
    })
}
