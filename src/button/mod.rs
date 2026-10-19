/// Convert native platform mouse button codes to [`Button`]s.
pub mod convert;

#[derive(Debug)]
/// A mouse button.
pub enum Button {
    Left,
    Right,
    Middle,
    Forward,
    Back,
    // TODO: should be u16?
    Other(u8),
}
