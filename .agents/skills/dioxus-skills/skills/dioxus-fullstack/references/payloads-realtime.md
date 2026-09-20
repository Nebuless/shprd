# Payloads and realtime transports

Source pin: `https://github.com/DioxusLabs/dioxus.git@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Payload matrix

| Need | Signature type | Wire | Critical edge |
| --- | --- | --- | --- |
| Ordinary RPC | serializable arguments/return | JSON | deployed-client schema compatibility |
| URL query | route `?name`, rename, or `?{query}` | query string | optionality and encoding |
| URL form | `Form<T>` | form-urlencoded | CSRF on cookie-authenticated mutation |
| Multipart | `MultipartFormData` | multipart | body extractor last |
| File | `FileStream` | streamed body and metadata | untrusted filename; limits |
| Typed stream | `Streaming<T, E>` | HTTP body stream | browser/proxy support |
| Events | `ServerEvents<T>` | SSE | one-way text framing |
| Duplex | `Websocket<In, Out, E>` | upgrade | upgrade auth and authorization |
| Header | `SetHeader<T>` | response header | browser visibility/cookie flags |
| Redirect | `Redirect` | 3xx `Location` | RPC does not navigate SPA |

Runtime implementations: `packages/fullstack/src/payloads/`. Example compositions: `examples/07-fullstack/`.

## Query, headers, and forms

- Route query supports direct parameters, `?external=rust_name`, and `?{struct}` catch-all. Share serializable/deserializable types.
- Extra macro arguments are server-only Axum extractors: `HeaderMap`, `TypedHeader<Cookie>`, `Query<T>`, and custom extractors.
- `Form<T>` delegates server extraction to Axum and builds client request through `IntoRequest`.
- `SetHeader<T>` appends typed output headers. Browser JS cannot read non-safelisted cross-origin headers unless CORS exposes them. Browser manages `Set-Cookie`.
- Cookies need `HttpOnly`, `Secure`, suitable `SameSite`, path/domain, rotation, and expiration. Login example is not production session design.

Runtime evidence: `packages/fullstack/src/payloads/query.rs`, `form.rs`, `header.rs`, and `request.rs:111-130`. Example evidence: `examples/07-fullstack/query_params.rs`, `header_map.rs`, `login_form.rs`.

## Multipart and files

- `MultipartFormData` converts browser form data into multipart request and exposes Axum fields on server.
- Iterate fields as stream. `field.bytes()` buffers full field; use chunk access for large uploads.
- `FileStream` carries filename, content type, size, and stream. Treat metadata as attacker-controlled.
- Replace/canonicalize filenames, write under controlled directory, reject traversal, cap bytes, and inspect actual content where required.
- Stream to temporary file, handle partial writes, verify actual size, atomically promote, and clean abandoned files.
- Browser `ByteStream` may buffer full file in WASM. Prefer native file transport where available.

Runtime evidence: `packages/fullstack/src/payloads/multipart.rs:15-102`; `packages/fullstack/src/payloads/files.rs:15-263`; `packages/fullstack/src/client.rs:184-220`. Example evidence: `examples/07-fullstack/multipart_form.rs:46-67`; `streaming_file_upload.rs:112-205`.

## HTTP streams

- `Streaming<T, E>` works as request and response. Aliases include `TextStream`, `ByteStream`, JSON/CBOR streams, and framed chunked streams.
- Text/byte chunk boundaries are not message boundaries unless encoding frames them.
- Browser request streams vary and implementation requires HTTP/2 or HTTP/3. Test target browsers and proxy.
- Client cancellation drops stream; producer should stop when send fails. Bound queues for slow consumers.
- Disable proxy buffering and tune timeouts for long responses. Compression can delay tiny chunks.

Runtime evidence: `packages/fullstack/src/payloads/stream.rs:19-140,238-378`. Example evidence: `examples/07-fullstack/streaming.rs:29-147`.

## SSE

- `ServerEvents<T>` puts JSON payloads in SSE text frames and validates `text/event-stream` client-side.
- SSE is server-to-client only. Use RPC for client commands.
- Send failure signals disconnect; stop producer and release subscriptions.
- Add keep-alive for idle intermediaries. Add event IDs, retry, and replay if reconnect loss matters; typed stream is not durable delivery.
- Use byte streams or WebSocket for binary-heavy traffic.

Runtime evidence: `packages/fullstack/src/payloads/sse.rs:16-120,263-384`. Example evidence: `examples/07-fullstack/server_sent_events.rs:32-95`.

## WebSockets

- Endpoint accepts `WebSocketOptions` and returns `Websocket<In, Out, Encoding>`. JSON is default; CBOR supports binary.
- Server `on_upgrade` owns typed socket. Client `use_websocket` wraps connect/reconnect, state, send, and receive.
- Reactive values captured by hook factory can trigger reconnect. Keep dependencies narrow.
- Authenticate origin/session during upgrade. Reauthorize messages for changing resources. Limit frame size, rate, and connection count.
- Dioxus custom server supports upgrades; reverse proxy must preserve upgrade headers and long idle timeout.
- Define heartbeat, backoff, resubscription, duplicate handling, and shutdown in app protocol.

Runtime evidence: `packages/fullstack/src/payloads/websocket.rs:4-160,488-1100`; upgrade server: `packages/fullstack-server/src/launch.rs:176-217`. Example evidence: `examples/07-fullstack/websocket.rs:26-100`.

## Redirects

- Axum `Redirect` maps supported status and `Location` through `FromResponse`.
- Redirect-returning RPC does not navigate SPA. Navigate through client router or ordinary browser form/request semantics.
- Allowlist or enforce same-origin for user-controlled destination.

Runtime evidence: `packages/fullstack/src/payloads/axum_types.rs:37-52`. Example evidence: `examples/07-fullstack/redirect.rs`.
