mod common;

use futures_util::StreamExt;

fn main() {
    let mut timer = smol::Timer::interval(common::TICK_DURATION).fuse();
    smol::block_on(async {
        let mut stream = antmon::listen::<byor::runtime::Smol>().await.unwrap();
        loop {
            futures_util::select! {
                _ = timer.next() => println!("tick"),
                r = stream.next() => println!("{:?}", r.unwrap()),
            }
        }
    })
}
