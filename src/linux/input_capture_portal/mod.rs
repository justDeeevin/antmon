use byor::executor::RuntimeExecutor;
use futures_util::StreamExt;
use reis::ei::Context;
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, sync::OnceLock};
use types::*;
use zbus::{
    DynamicType, OwnedFd, OwnedObjectPath, Proxy, Type, Variant, connection::Builder,
    names::MemberName,
};
use zbus_compat::ZbusRt;

mod types;
mod zbus_compat;

pub static RESTORE_TOKEN: OnceLock<String> = OnceLock::new();
type Dict<'a> = HashMap<String, Variant<'a>>;

async fn setup<Rt: RuntimeExecutor>() -> zbus::Result<()>
where
    zbus_compat::ZbusRt<Rt>: zbus::runtime::traits::Runtime,
{
    let connection = Builder::session().runtime(ZbusRt::new()?).build().await?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.InputCapture",
    )
    .await?;

    let token = HandleToken::default();
    let CreateSession2Results { session_handle } = proxy
        .call(
            "CreateSession2",
            &CreateSession2Options {
                session_handle_token: token.clone(),
            },
        )
        .await?;

    let res = get_response::<StartResponse>(
        &proxy,
        "Start",
        &(
            &session_handle,
            "",
            StartOptions {
                handle_token: token.clone(),
                capabilities: SupportedCapabilities::KEYBOARD | SupportedCapabilities::POINTER,
                restore_token: RESTORE_TOKEN.get().cloned(),
                persist_mode: Some(PersistMode::UntilRevocation),
            },
        ),
    )
    .await?;

    let eis: OwnedFd = proxy
        .call("ConnectToEIS", &(&session_handle, Dict::default()))
        .await?;
    let context = Context::new(std::os::fd::OwnedFd::from(eis).into())?;

    let zones =
        get_response::<GetZonesResponse>(&proxy, "GetZones", &(&session_handle, Dict::default()))
            .await?;

    todo!()
}

async fn get_response<T: Type + DeserializeOwned>(
    proxy: &Proxy<'_>,
    method: impl TryInto<MemberName<'_>, Error: Into<zbus::Error>>,
    body: &(impl Serialize + DynamicType),
) -> zbus::Result<Response<T>> {
    let handle: OwnedObjectPath = proxy.call(method, body).await?;
    Proxy::new(
        proxy.connection(),
        "org.freedesktop.portal.Desktop",
        handle,
        "org.freedesktop.portal.Request",
    )
    .await?
    .receive_signal("Response")
    .await?
    .next()
    .await
    .unwrap()
    .body()
    .deserialize()
}
