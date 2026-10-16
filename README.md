# Topcoat + iroh

**Your existing Topcoat app, reachable from anywhere.**

A fork of [Topcoat](https://github.com/tokio-rs/topcoat) designed as a **drop-in iroh integration for existing Topcoat apps**. You should be able to point your app at this fork, run `topcoat bundle`, run the bundled server, and deploy the generated launcher. Your entire app should then be accessible from anywhere with relay access, with pages, app assets, requests, and live updates all routed through iroh relays.

Keep your application state, logic, and rendering on your own machine, without exposing an HTTP port, setting up a VPN, or managing a tunnel.

Built for personal tools, home servers, live dashboards, and shared apps where the server owns the state.

## Differences

You deploy a **static launcher** alongside your Rust server. It loads the browser's JavaScript/Wasm transport and connects to the app over iroh. Optionally design an interactive endpoint, ticket, and secret connection screen with `#[launcher]`, in the same Cargo package.

The launcher automatically installs this fork's **service worker**, which forwards normal browser requests through iroh's Wasm client. Pages, assets, forms, and procedures keep their usual URLs while the worker handles the transport.

The application developer experience stays vanilla Topcoat: `view!`, pages, components, procedures, shards, `live!`, and `emit!`.

## Build and deploy

Point your `topcoat` dependency at this fork and keep `topcoat::start(router)`. Run `topcoat bundle` with this fork's CLI.

```text
topcoat bundle
  --iroh-secret KEY     # Embed a stable server key; derive the public endpoint (optional)
  --iroh-endpoint ID    # Set the public server endpoint (optional)
  --iroh-ticket TICKET  # Use a server ticket instead of --iroh-endpoint (optional)
  --iroh-peer ID        # Allow this client identity; repeat for more clients (optional)
```

The CLI builds the server, assets, and launcher. No custom `#[launcher]` is needed.

```text
target/release/
  YOUR_APP    Run on your server.
  assets/     Keep beside the executable.
  launcher/   Publish to a static HTTPS host.
```

Host `launcher/` at the root of a dedicated origin, such as a GitHub Pages user site or custom domain. Open that URL to connect. Both browser and server need relay access; the Rust app needs no public HTTP port. Ordinary `cargo run` and `topcoat dev` keep HTTP.

Common scenarios:

- **No switches:** `topcoat bundle` generates or reuses a key in `target/`, embeds it in the server, and configures the launcher. **Do not use this in production unless you keep the same key outside `target/` and supply it through `TOPCOAT_IROH_SECRET` at runtime.** Bundling warns that `cargo clean` permanently deletes the saved key. The environment variable is not read during bundling.
- **Secret only:** `topcoat bundle --iroh-secret KEY` embeds your stable server key and derives the launcher's public endpoint. Keep your production key outside `target/`.
- **Endpoint only:** `topcoat bundle --iroh-endpoint ID` embeds no key. Set `TOPCOAT_IROH_SECRET` on the server; startup fails if it is missing or does not match `ID`.
- **Ticket only:** `topcoat bundle --iroh-ticket TICKET` works like endpoint-only, using the ticket's endpoint and connection information. The runtime key must match the endpoint inside the ticket.
- **Secret plus endpoint or ticket:** add `--iroh-endpoint ID` or `--iroh-ticket TICKET` to `topcoat bundle --iroh-secret KEY`. The key is embedded, and bundling fails if the supplied destination does not match it.
- **Restricted access:** add `--iroh-peer CLIENT_ID` to any scenario, repeating it for more clients. Used alone, it keeps the automatic key behavior. Without a saved or supplied allowlist, all clients can connect.

`--iroh-endpoint` and `--iroh-ticket` are mutually exclusive. Server keys stay out of the launcher; `#secret` selects the browser's separate client identity. See the [launcher guide](docs/launcher.md) for key and allowlist persistence.

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
