# <ins>A</ins>sync I<ins>n</ins>pu<ins>t</ins> <ins>Mon</ins>itor

Antmon is an async library for cross-platform global input monitoring. It's also what they call Ant-Man in Jamaica 😝

It doesn't simply use a worker thread. It leverages file readininess notifications to entirely avoid blocking.

## Usage

Antmon provides an opaque stream of events that can be created with [`events`]:

```rust
let mut stream = antmon::events::<byor::runtime::Tokio>().await?;
while let Some(e) = stream.next().await {
    let _ = dbg!(e);
}
```

**Note:** Antmon uses [`byor`](https://lib.rs/crates/byor) to abstract over runtime-specific behavior[^1]. This allows consumers to select the runtime the prefer.

[^1]: It only needs filesystem behavior under the hood.
