use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use zbus::{
    DeserializeDict, OwnedObjectPath, SerializeDict, Type,
    as_variant::{self, optional},
};

#[derive(Serialize, Type, Clone)]
#[serde(transparent)]
pub struct HandleToken(String);

#[derive(Deserialize, Type)]
pub struct Response<T: Type> {
    pub response: ResponseStatus,
    pub results: T,
}

#[derive(Deserialize_repr, Type)]
#[repr(u32)]
pub enum ResponseStatus {
    Success,
    Cancelled,
    Other,
}

impl Default for HandleToken {
    fn default() -> Self {
        const ALPHANUMERIC: &[u8] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        const LIB: &str = "antmon";
        const SUFFIX_LEN: usize = 10;
        const TOKEN_LEN: usize = LIB.len() + SUFFIX_LEN + 1;

        let mut token = String::with_capacity(TOKEN_LEN);
        token += LIB;
        token.push('_');
        let mut rng = rand::rng();

        for _ in 0..SUFFIX_LEN {
            token.push(*ALPHANUMERIC.choose(&mut rng).unwrap() as char)
        }

        Self(token)
    }
}

#[derive(SerializeDict, Type, Default)]
#[zbus(signature = "a{sv}")]
pub struct CreateSession2Options {
    pub session_handle_token: HandleToken,
}

#[derive(DeserializeDict, Type)]
#[zbus(signature = "a{sv}")]
pub struct CreateSession2Results {
    pub session_handle: OwnedObjectPath,
}

#[derive(Serialize, Type)]
#[zbus(signature = "a{sv}")]
pub struct StartOptions {
    #[serde(with = "as_variant")]
    pub handle_token: HandleToken,
    #[serde(with = "as_variant")]
    pub capabilities: SupportedCapabilities,
    #[serde(with = "optional", skip_serializing_if = "Option::is_none")]
    pub restore_token: Option<String>,
    #[serde(with = "optional", skip_serializing_if = "Option::is_none")]
    pub persist_mode: Option<PersistMode>,
}

#[derive(Serialize_repr, Type)]
#[repr(u32)]
pub enum PersistMode {
    None,
    WhileRunning,
    UntilRevocation,
}

#[derive(Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct SupportedCapabilities(u32);

bitflags::bitflags! {
    impl SupportedCapabilities: u32 {
        const KEYBOARD = 1;
        const POINTER = 2;
        const TOUCHSCREEN = 4;
    }
}

#[derive(DeserializeDict, Type)]
#[zbus(signature = "a{sv}")]
pub struct StartResponse {
    pub capabilities: SupportedCapabilities,
    pub clipboard_enabled: bool,
    pub restore_token: String,
}

#[derive(DeserializeDict, Type)]
#[zbus(signature = "a{sv}")]
pub struct GetZonesResponse {
    pub zones: Vec<Zone>,
    pub zone_set: u32,
}

#[derive(Deserialize, Type)]
#[zbus(signature = "(uuii)")]
pub struct Zone {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
}
