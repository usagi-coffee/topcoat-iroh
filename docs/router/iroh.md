The `iroh` feature serves a Topcoat app to a statically hosted browser launcher. The static host delivers the launcher, service worker, and Wasm bindings. The browser then requests the app's HTML, assets, forms, navigation, and procedures through iroh. The Rust process needs no public HTTP listener or inbound port forwarding.

Browsers reach iroh peers through relays. App traffic is encrypted between the browser and the Rust endpoint. The static host supplies executable code and must be trusted. Both peers need access to the configured relays.

# Server

Enable `iroh` and `runtime-iroh` on your `topcoat` dependency and add `iroh = "1.3"`. Register `IrohService` for app requests and `IrohTransport` for connected renders on the same endpoint:

```rust,no_run
# async fn example() -> topcoat::Result<()> {
use iroh::{Endpoint, endpoint::presets};
use topcoat::{
    asset::{AssetBundle, RouterBuilderAssetExt},
    router::{IROH_HTTP_ALPN, IrohService, Router},
    runtime::{IROH_RUNTIME_ALPN, IrohTransport, RouterBuilderRuntimeExt},
};

let endpoint = Endpoint::bind(presets::N0).await?;
let transport = IrohTransport::new(
    endpoint.clone(),
    "/__topcoat/topcoat_runtime_iroh_browser.js",
);
let app = Router::builder()
    // Register application pages, routes, and shards here.
    .assets(AssetBundle::load()?)
    .app_context(transport.clone())
    .runtime()
    .build();
let listener = iroh::protocol::Router::builder(endpoint.clone())
    .accept(IROH_HTTP_ALPN, IrohService::new(app))
    .accept(IROH_RUNTIME_ALPN, transport)
    .spawn();

endpoint.online().await;
println!("App address: {}", endpoint.id());
tokio::signal::ctrl_c().await?;
listener.shutdown().await?;
# Ok(())
# }
```

Include `topcoat::runtime::script()` in interactive pages. The runtime's bootstrap request also goes through the launcher, so live rendering does not require an HTTP server. Ordinary views and routes need no runtime script.

Configure the endpoint with a persisted secret key if its address must survive restarts. Its public endpoint ID is an address, not an application credential. Routes and layers must authorize requests as usual. `request_context::<IrohPeer>(cx)` exposes the authenticated peer on requests served by `IrohService`.

# Static Launcher

From the Topcoat checkout, install the Wasm target and the `wasm-bindgen` CLI matching `Cargo.lock`, then build:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
bash scripts/build-iroh-launcher.sh
```

Upload `target/iroh-launcher/` to the root of a dedicated HTTPS origin. A GitHub Pages user or organization site, or a custom domain, works. Project sites under `/repository/` are not supported because the worker must control root-relative app URLs. The generated `404.html` bootstraps first visits to deep links on GitHub Pages. Other hosts should serve the launcher for missing document paths.

Set `__topcoat/config.json` to `{"endpoint":"YOUR_PUBLIC_ENDPOINT_ID"}` for automatic connection. The build script preserves an existing config. You can also open the launcher with `#iroh=YOUR_PUBLIC_ENDPOINT_ID` or enter the address in its form. The worker saves the endpoint and reconnects when it restarts. Open `/__topcoat/index.html` to change the address.

The `/__topcoat/` namespace and `/topcoat-sw.js` belong to the launcher. Other same-origin requests go to the app through Wasm. External URLs remain ordinary browser requests. Failed app requests are never retried against the static host. Failed mutations are not automatically replayed.

Deploy matching JavaScript and Wasm bindings together. If you configure a content security policy, allow the worker, Wasm execution, and relay connections. Service workers require HTTPS, except on localhost.

# Sessions

Synthetic service worker responses cannot set browser HTTP cookies. The launcher rejects `Set-Cookie` responses instead of silently discarding authentication state. Use `HeaderTokenStore` for sessions:

```rust
use topcoat::{
    router::Router,
    session::{HeaderTokenStore, RouterBuilderSessionExt, SessionConfig},
};

let app = Router::builder()
    .sessions(SessionConfig::builder().token_store(HeaderTokenStore).build())
    .build();
```

The launcher stores tokens in `IndexedDB` and attaches them as bearer credentials, including on navigation and runtime bootstrap requests. Session creation, renewal, and deletion update storage before the response reaches the page. Application code must still persist and validate session records and expiration.

`IndexedDB` does not provide `HttpOnly` protection. Scripts on the origin can read its session storage. Use a dedicated origin for each trusted app. Do not use one launcher origin to run untrusted peers. Cookie-based authentication integrations need a different deployment model.

# Streaming And Limits

Each request uses a separate bidirectional stream. Request and response metadata use a four-byte big-endian length followed by UTF-8 JSON, limited to 64 KiB. Bodies stream until EOF. Request headers must arrive within 30 seconds. The service allows 128 concurrent requests per connection by default; configure this with `IrohService::max_requests`. Router body limits still apply.

Cancelling a request cancels its server work. Disconnecting cancels the connection's requests. Subsequent requests establish a new connection. Binary assets, redirects, uploads, and streaming responses use the same path.

Raw WebSocket upgrades are unsupported. Topcoat connected renders use their separate iroh protocol. The launcher requests uncompressed bodies because constructing a browser `Response` does not perform HTTP content decoding.

# Example

The `iroh-app` example has no HTTP listener. It demonstrates pages, a bundled stylesheet, a procedure, a shard, live updates, binary uploads, and sessions.

```sh
cargo topcoat asset bundle -p iroh-app
cargo run -p iroh-app
```

In another terminal, build the launcher as above. For local testing, serve only its static output:

```sh
python3 -m http.server 8080 --directory target/iroh-launcher
```

Open `http://localhost:8080/#iroh=YOUR_PUBLIC_ENDPOINT_ID` with the address printed by the example. For access from another network, publish the same output at your HTTPS origin. The Rust app can remain on a private network.

To run the browser smoke test, start Chromium with a fresh profile and remote debugging, then run the test with Node.js 22 or newer:

```sh
chromium --headless --remote-debugging-port=9398 --user-data-dir=/tmp/topcoat-iroh-test about:blank
IROH_TEST_URL=http://localhost:8080 IROH_TEST_PEER=YOUR_PUBLIC_ENDPOINT_ID node scripts/test-iroh-launcher.mjs
```

The test checks initial HTML, assets, procedures, shards, live updates, uploads, form redirects, navigation, and session persistence after a service worker restart. Run the commands in separate terminals while the app and static server remain running.
