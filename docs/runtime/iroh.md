The optional iroh transport carries connected page and shard renders over [iroh](https://iroh.computer). Existing `live!`, `emit!`, and `connected(cx)` calls work unchanged. Initial pages, assets, navigation, and procedure calls still use HTTP.

To load the entire app from a static host while the Rust server runs on a private network, use the [static iroh launcher](https://github.com/tokio-rs/topcoat/blob/main/docs/router/iroh.md). Its Wasm transport also carries the initial page, assets, ordinary requests, and this transport's bootstrap.

Enable `runtime-iroh` in your `topcoat` dependency and add `iroh = "1.3"`. Use a native Rust server. Browsers run the transport through a separate Wasm module and connect through iroh relays. The relay cannot decrypt the connection. Native iroh clients can establish direct connections.

# Browser Module

Build the browser bindings from the Topcoat checkout. Install the Wasm target and the `wasm-bindgen` CLI version matching `wasm-bindgen` in `Cargo.lock`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo build -p topcoat-runtime-iroh-browser --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir examples/iroh-live/public/iroh target/wasm32-unknown-unknown/release/topcoat_runtime_iroh_browser.wasm
```

Serve the generated `.js` and `.wasm` files together. The JavaScript module resolves the Wasm file relative to its own URL. Build and deploy matching files whenever the bindings change. Allow the module, Wasm execution, and relay connections in your application's content security policy if you configure one.

# Server Setup

Create an endpoint and register `IrohTransport` with both routers. The Topcoat router stores the transport in its app context. The iroh protocol router accepts connections for `IROH_RUNTIME_ALPN`.

```rust,no_run
# async fn example() -> topcoat::Result<()> {
use iroh::{Endpoint, endpoint::presets};
use topcoat::{
    asset::{AssetBundle, RouterBuilderAssetExt},
    router::{Router, RouterBuilderDirectoryExt},
    runtime::{IROH_RUNTIME_ALPN, IrohTransport, RouterBuilderRuntimeExt},
};

let endpoint = Endpoint::bind(presets::N0).await?;
let transport = IrohTransport::new(
    endpoint.clone(),
    "/iroh/topcoat_runtime_iroh_browser.js",
);
let listener = iroh::protocol::Router::builder(endpoint)
    .accept(IROH_RUNTIME_ALPN, transport.clone())
    .spawn();

let router = Router::builder()
    // Register your pages and shards here.
    .assets(AssetBundle::load()?)
    .serve_dir("/iroh/{*rest}", "public/iroh")
    .app_context(transport)
    .runtime()
    .build();

let result = topcoat::start(router).await;
listener.shutdown().await?;
result?;
# Ok(())
# }
```

Enable `fs` to use `serve_dir`. Include `topcoat::runtime::script()` in the document. It selects iroh when the transport is present in app context and loads the bindings when a page or shard requests a connection. Without this context value, the runtime uses WebSocket.

Keep the iroh protocol router alive while serving the application. You can configure the endpoint's identity, relays, and address lookup through iroh before creating the transport. All Topcoat server instances handling bootstrap requests must reach the same ticket store as their advertised iroh endpoint. A transport's tickets live in its process.

The [iroh live example](https://github.com/tokio-rs/topcoat/tree/main/examples/iroh-live) emits a counter once per second. After generating its browser files, run `topcoat dev` from `examples/iroh-live` and open the address it prints.

# Authentication And Reconnects

The browser creates an ephemeral iroh identity and makes an HTTP `POST` to the current page URL with `X-Topcoat-Iroh: true`. The response contains the server address and a single-use ticket bound to that browser identity. The request uses the browser's cookies and the router's origin policy. Tickets expire after 30 seconds, and at most 1024 unclaimed tickets are retained per transport.

The browser presents the ticket over iroh. Connected renders inherit the bootstrap request's headers, including cookies. Run messages can only supply content type and Topcoat's rerender headers. They cannot replace credentials or the origin. Each render still passes through the application's routes and layers, so authorize pages, shards, and procedures as usual.

One bidirectional stream carries the document's render requests and output. Messages use a four-byte big-endian byte length followed by UTF-8 JSON, with an 8 MiB limit per message. The first message is `{"ticket":"..."}`. Later messages use the runtime's run and stop protocol. `max_runs_per_connection` applies to iroh and WebSocket alike.

Disconnecting cancels the connection's renders. A reconnect obtains a new ticket with current HTTP credentials and reruns each connected page or shard with current inputs and signals. The live body starts again, so read current application state on each run and start durable work outside the render. See the [live guide](https://docs.rs/topcoat/latest/topcoat/view/macro.live.html).
