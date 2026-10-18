use collaboration_central_core::*;
use serde::{Deserialize, Serialize};
use worker::{Env, Headers, Method, Request, RequestInit, Response, Result, Stub};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum RpcIdentity {
    Edge,
    Admin,
    Object(ObjectAddress),
    #[cfg(feature = "conformance")]
    Conformance,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectRequest {
    pub address: ObjectAddress,
    pub caller: RpcIdentity,
    pub caller_kind: CallerKind,
    #[cfg(feature = "conformance")]
    pub test_clock: Option<RegistryTime>,
    pub input: WireInput,
    pub now: RegistryTime,
}
pub(crate) fn stub(env: &Env, address: &ObjectAddress) -> Result<Stub> {
    let (binding, name) = match address {
        ObjectAddress::Copy(project) => ("PROJECT_COPIES", project.as_str()),
        ObjectAddress::Connection(installation) => ("CONNECTIONS", installation.as_str()),
        ObjectAddress::Registry => ("LEASE_REGISTRY", "registry"),
        ObjectAddress::Fleet => ("FLEET", "fleet"),
    };
    env.durable_object(binding)?.id_from_name(name)?.get_stub()
}
pub(crate) async fn request(
    env: &Env,
    address: &ObjectAddress,
    path: &str,
    body: &impl Serialize,
) -> Result<Response> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_body(Some(serde_json::to_string(body)?.into()));
    let headers = Headers::new();
    headers.set("content-type", "application/json")?;
    init.with_headers(headers);
    stub(env, address)?
        .fetch_with_request(Request::new_with_init(
            &format!("https://central.internal{path}"),
            &init,
        )?)
        .await
}
pub(crate) async fn decode<TValue: serde::de::DeserializeOwned>(
    mut response: Response,
) -> Result<TValue> {
    if response.status_code() != 200 {
        return Err(worker::Error::RustError(format!(
            "object response status {}",
            response.status_code()
        )));
    }
    Ok(serde_json::from_str(&response.text().await?)?)
}

/// The production object client cannot issue an unranked edge.
pub(crate) async fn invoke<From: MayCall<To>, To: WireKind>(
    context: &crate::object_context::ObjectContext,
    call: Dispatch<To>,
) -> Result<To::Result> {
    #[cfg(feature = "conformance")]
    crate::conformance_control::remember_outgoing::<From, To>(context, &call.to).await?;
    let address = To::address(&call.to);
    let request_body = ObjectRequest {
        address: address.clone(),
        caller: RpcIdentity::Object(context.address.clone()),
        caller_kind: From::KIND,
        #[cfg(feature = "conformance")]
        test_clock: context.storage.get("_cf_test_clock").await?,
        input: WireInput::Call {
            from: context.address.clone(),
            call_id: call.call_id,
            call: Box::new(To::call(call.call)),
        },
        now: context.now().await?,
    };
    let result = decode(request(&context.env, &address, "/invoke", &request_body).await?).await?;
    To::decode_result(result).map_err(|error| worker::Error::RustError(error.to_string()))
}
