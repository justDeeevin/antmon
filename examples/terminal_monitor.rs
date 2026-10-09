use futures_util::StreamExt;

fn main() {
    let mut stream = antmon::listen::<byor::runtime::Smol>().unwrap();
    smol::block_on(async {
        while let Some(event) = stream.next().await {
            println!("{event:?}");
        }
    });
}
