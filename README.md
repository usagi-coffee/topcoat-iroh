# Topcoat + iroh

**Live apps, wherever your server lives.**

A fork of [Topcoat](https://github.com/tokio-rs/topcoat) for fully server-side live apps, accessible across networks. Keep your application state, logic, and rendering on your own machine. Open the app in a browser from anywhere with access to iroh relays, without exposing an HTTP port, setting up a VPN, or managing a tunnel.

Built for personal tools, home servers, live dashboards, and shared apps where the server owns the state.

## Differences

You deploy a **static launcher** alongside your Rust server. It loads the browser's JavaScript/Wasm transport and connects to the app over iroh. Optionally design an interactive endpoint, ticket, and secret connection screen with `#[launcher]`, in the same Cargo package.

The application developer experience stays vanilla Topcoat: `view!`, pages, components, procedures, shards, `live!`, and `emit!`.

## Build and deploy

Point your existing `topcoat` dependency at this fork and keep `topcoat::start(router)` as usual. Use this fork's CLI:

```sh
topcoat bundle
```

```text
target/release/
  YOUR_APP    Run this executable on your server.
  assets/     Keep beside the executable.
  launcher/   Publish these static files to an HTTPS host.
```

A GitHub Pages user site or custom domain works. Host at the root of a dedicated origin; project subpaths are not supported. Your domain points to the static host, and your Rust app stays on its own network.

The bundle embeds a server identity and configures the launcher with its public endpoint, so it is ready to connect. Ordinary `cargo run` and `topcoat dev` keep HTTP.

Use `--iroh-secret SERVER_KEY` to embed a stable production identity. To keep the key out of the build, bundle with `--iroh-endpoint SERVER_ID` or `--iroh-ticket SERVER_TICKET` and set `TOPCOAT_IROH_SECRET` on the server. A supplied key must match that destination. Repeat `--iroh-peer CLIENT_ID` to restrict access to listed clients.

With none of these destination or secret switches, the CLI generates or reuses a key in `target/` and warns that `cargo clean` permanently removes the saved key. Keep a stable production key outside the build directory.

The browser creates its own identity, or uses a client key from `#secret=...`. Links can override the server with `#endpoint=...` or `#ticket=...`; combine fragment fields with `&`.

## Make the welcome screen yours (optional)

Add `#[launcher]` in `main.rs` or any module beside your app code. The CLI detects it automatically, and `/` remains an application route.

```rust
use topcoat::{
    Result, context::Cx, launcher,
    runtime::{Event, expr, signal},
    view::{View, view},
};

#[launcher]
async fn welcome(cx: &Cx) -> Result<impl View> {
    let endpoint = signal(cx, launcher::endpoint);
    let ticket = signal(cx, launcher::ticket);
    let secret = signal(cx, String::new);
    let status = signal(cx, String::new);
    let connect = launcher::connect(&expr!(endpoint.get()), &expr!(ticket.get()), &expr!(secret.get()), &status);

    Ok(view! {
        <!DOCTYPE html>
        <html>
            <head>launcher::script()</head>
            <body>
                <form @submit=(connect)>
                    <label>
                        "Server endpoint (optional override)"
                        <input :value=$(endpoint.get()) @input=$(|e: Event| endpoint.set(e.target.value))>
                    </label>
                    <label>
                        "Server ticket (instead of endpoint)"
                        <input :value=$(ticket.get()) @input=$(|e: Event| ticket.set(e.target.value))>
                    </label>
                    <label>
                        "Client secret (optional)"
                        <input type="password" :value=$(secret.get()) @input=$(|e: Event| secret.set(e.target.value))>
                    </label>
                    <button type="submit">"Open my app"</button>
                    <p role="status">$(status.get())</p>
                </form>
            </body>
        </html>
    })
}
```

For a custom launcher, enable the `launcher` feature and call `launcher::export_if_requested().await?` before server startup, returning from `main` when it returns `true`. This lets the CLI render the screen at build time.

The submit button runs `launcher::connect` in the browser. `launcher::endpoint()` and `launcher::ticket()` prefill the public destination. You can make a prefilled input read-only with `readonly=(!launcher::endpoint().is_empty())`. Empty destination inputs use the bundled server. Design this screen as you would any Topcoat view. Once connected, the browser opens your normal app. Without `#[launcher]`, neither the feature nor the export hook is needed.

[Launcher guide](docs/launcher.md) covers deployment and connection links. [Transport guide](docs/router/iroh.md) covers custom server setups. [REBASE.md](REBASE.md) records the integration and upstream rebase workflow.
