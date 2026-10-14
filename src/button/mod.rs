pub mod convert;

#[derive(Debug)]
pub enum Button {
    Left,
    Right,
    Middle,
    Forward,
    Back,
    // TODO: should be u16?
    Other(u8),
}
