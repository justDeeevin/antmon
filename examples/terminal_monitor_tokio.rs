mod common;

use futures_util::StreamExt;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut stream = antmon::listen::<byor::runtime::Tokio>().unwrap();
    let mut timer = tokio::time::interval(common::TICK_DURATION);
    loop {
        tokio::select! {
            _ = timer.tick() => println!("tick"),
            Some(Ok(e)) = stream.next() => println!("{e:?}")
        }
    }
}
