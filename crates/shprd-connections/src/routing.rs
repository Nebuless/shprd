use crate::{ConnectionId, Error, Lease, Manager, Result};
use serde_json::{Map, Value, json};
pub(crate) fn routing(status: u16, message: &str) -> Error {
    Error::Routing {
        status,
        message: message.into(),
    }
}
pub fn generation(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .filter(|g| *g <= 9_007_199_254_740_991)
        .ok_or_else(|| routing(400, "invalid connection_generation"))
}
pub fn query_generation(value: Option<&str>) -> Result<Option<u64>> {
    value
        .map(|s| {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(routing(400, "invalid connection_generation"));
            }
            let n = s
                .parse::<u64>()
                .map_err(|_| routing(400, "invalid connection_generation"))?;
            generation(&json!(n))
        })
        .transpose()
}
pub enum RpcRoute {
    Bridge,
    Connection(Lease),
}
pub fn resolve_rpc(manager: &Manager, request: &Value) -> Result<RpcRoute> {
    let object = request
        .as_object()
        .ok_or_else(|| routing(400, "invalid request envelope"))?;
    let id = object.get("id").and_then(Value::as_str);
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| routing(400, "missing id/method"))?;
    if id.is_none_or(str::is_empty) || method.is_empty() {
        return Err(routing(400, "missing id/method"));
    }
    if method.starts_with("bridge.") || method.starts_with("connections.") {
        if object.contains_key("connection_id") || object.contains_key("connection_generation") {
            return Err(routing(
                400,
                "bridge-global method must not include connection identity",
            ));
        }
        return Ok(RpcRoute::Bridge);
    }
    let id = object
        .get("connection_id")
        .map(|v| {
            v.as_str()
                .ok_or_else(|| routing(400, "invalid connection_id"))
                .and_then(ConnectionId::parse)
        })
        .transpose()?;
    let expected = object
        .get("connection_generation")
        .map(generation)
        .transpose()?;
    manager
        .resolve(id.as_ref(), expected)
        .map(RpcRoute::Connection)
}
pub fn envelope(
    id: &ConnectionId,
    generation: Option<u64>,
    mut message: Map<String, Value>,
) -> Result<Value> {
    if message.contains_key("connection_id") || message.contains_key("connection_generation") {
        return Err(routing(
            400,
            "connection envelope payload contains reserved identity",
        ));
    }
    message.insert("connection_id".into(), json!(id));
    if let Some(g) = generation {
        if g > 9_007_199_254_740_991 {
            return Err(routing(400, "invalid connection_generation"));
        }
        message.insert("connection_generation".into(), json!(g));
    }
    Ok(message.into())
}
pub fn event_envelope(id: &ConnectionId, generation: Option<u64>, event: Value) -> Result<Value> {
    let object = event
        .as_object()
        .ok_or_else(|| routing(400, "invalid Herdr event envelope"))?;
    if !object.get("event").is_some_and(Value::is_string) {
        return Err(routing(400, "invalid Herdr event envelope"));
    }
    for field in [
        "connection_id",
        "connection_generation",
        "hello",
        "id",
        "result",
        "error",
        "control",
        "terminal",
        "terminal_clipboard",
    ] {
        if object.contains_key(field) {
            return Err(routing(400, "Herdr event contains reserved field"));
        }
    }
    envelope(id, generation, object.clone())
}
pub struct ReplyPublisher {
    lease: Lease,
    request_id: String,
    stale_sent: bool,
}
impl ReplyPublisher {
    pub fn new(lease: Lease, request_id: String) -> Self {
        Self {
            lease,
            request_id,
            stale_sent: false,
        }
    }
    pub fn publish(&mut self, message: Map<String, Value>) -> Result<Option<Value>> {
        let payload = if self.lease.is_current() {
            message
        } else {
            if self.stale_sent {
                return Ok(None);
            }
            self.stale_sent = true;
            Map::from_iter([
                ("id".into(), json!(self.request_id)),
                (
                    "error".into(),
                    json!({"message":"connection changed during request"}),
                ),
            ])
        };
        envelope(
            &self.lease.connection_id,
            Some(self.lease.generation()),
            payload,
        )
        .map(Some)
    }
}
