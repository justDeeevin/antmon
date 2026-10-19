mod common;

use futures_util::StreamExt;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut stream = antmon::events::<byor::runtime::Tokio>().await.unwrap();
    let mut timer = tokio::time::interval(common::TICK_DURATION);
    loop {
        tokio::select! {
            _ = timer.tick() => println!("tick"),
            Some(e) = stream.next() => println!("{e:?}")
        }
    }
}
