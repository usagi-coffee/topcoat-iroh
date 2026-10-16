The CLI builds a static iroh client and a Rust server from one application package, keeping ordinary `topcoat::start(router)` application code. A custom connection screen is optional. The server owns application state and keeps its normal routes, including `/`.

# Build And Deploy

Install this fork's CLI from its checkout with `cargo install --path crates/topcoat-cli --locked`. Point the application's `topcoat` dependency at this fork, keep its default features, and run:

```sh
topcoat bundle
```

The CLI enables iroh startup in `topcoat::start` and includes a static launcher. It also supports apps that register their own `IrohService`. It exports a compiled `#[launcher]` view when present, or generates a small connection bootstrap with no form. No particular source filename, separate binary, or launcher flag is required. Apps without iroh get only the server and its assets.

The CLI generates a server key, embeds it in the native executable, and writes only its public endpoint to `launcher/__topcoat/config.json`. For a stable production identity, pass `--iroh-secret SERVER_KEY`. Repeat `--iroh-peer CLIENT_ID` to allow only listed clients. These options configure the server, never the browser's private identity. Apps without a custom launcher need neither the `launcher` feature nor an export hook.

The default profile is `release`. Use `--profile dev` for a local build, `--package NAME` in a workspace, `--bin NAME` to select an executable, and `--out PATH` to choose the output directory. The default output is:

```text
target/release/
  PACKAGE
  assets/
  launcher/
    index.html
    404.html
    topcoat-sw.js
    __topcoat/
```

The native executable stays in Cargo's profile directory. `--profile dev` uses `target/debug/`; a custom `--out PATH` copies the executable and assets into that directory with a sibling `launcher/`. Run the server executable with its `assets/` directory beside it. Publish `launcher/` at the root of a dedicated HTTPS origin. GitHub Pages with a custom domain or a user site works; repository subpaths are not supported. The server needs relay connectivity, but no public HTTP listener. The build stages assets and launcher files before replacing them and refuses to replace an unrelated launcher directory. It leaves other files in Cargo's profile directory alone. Keep application data outside the generated output.

The CLI builds the browser transport and generates matching JavaScript and Wasm bindings. It installs the Rust Wasm target if missing. There is no separate `wasm-bindgen` CLI to install and no source checkout required after installing the Topcoat CLI. The first build needs access to Rust's download servers and Cargo dependencies. Later builds reuse Cargo's cache.

# Standard Server Startup

The fork enables the `iroh` feature by default. `topcoat bundle` compiles `topcoat::start(router)` to serve pages, assets, procedures, shards, and connected renders over iroh, with no HTTP listener. Ordinary `cargo run` and `topcoat dev` retain their HTTP behavior. Explicit `topcoat::serve` calls retain the listener you supply; use the [transport guide](router/iroh.md) for custom serving arrangements.

The server prints its endpoint and ticket when online. The generated launcher connects to this endpoint automatically. A custom launcher stays on its connection screen unless a connection fragment is supplied. Empty endpoint and ticket inputs in `launcher::connect` use the bundled destination. Browser links can still select a different endpoint or ticket.

Build settings are saved in `target/topcoat/identities/PACKAGE/BINARY.json`, with private file permissions on Unix. Later bundles reuse the server key and allowlist. An explicit `--iroh-secret` replaces the key; one or more `--iroh-peer` values replace the allowlist. Without a saved or supplied allowlist, all peers are allowed. To clear a saved list, set `peers` to `[]` in that build file and bundle again.

Without `--iroh-secret`, `--iroh-endpoint`, or `--iroh-ticket`, bundling warns that `cargo clean` or deleting the build directory permanently removes the saved key. Use a stable production key and keep your own copy outside `target/`. Rebuilding with a different key changes the public endpoint and requires deploying the new launcher. An embedded key can be extracted from the executable. No private server configuration is uploaded with `launcher/` or placed in public assets.

To supply the key only at deployment, bundle with `--iroh-endpoint SERVER_ID` or `--iroh-ticket SERVER_TICKET`, then set `TOPCOAT_IROH_SECRET` when running the server. These two switches are mutually exclusive. Without `--iroh-secret`, this mode neither reads nor updates the private identity cache, embeds no key, and emits no generated-key warning. Supply any `--iroh-peer` restrictions on each such bundle. The server refuses to start if the environment key is missing, invalid, or derives a different endpoint from the bundled destination. The CLI does not read `TOPCOAT_IROH_SECRET` to embed it.

When `--iroh-secret` is supplied alongside a destination, bundling fails if the key does not match its endpoint, including the endpoint inside a ticket. At runtime, `TOPCOAT_IROH_SECRET` takes precedence over an embedded key and must still match the bundled endpoint.

The allowlist checks the authenticated browser app peer before handling requests or issuing live-render credentials. Live rendering uses a separate runtime endpoint, authorized by a short-lived, single-use ticket bound to that endpoint. An unlisted app peer cannot obtain that ticket. The client key supplied through `#secret` or the launcher form is separate from `--iroh-secret`.

# Custom Connection Screen

Enable Topcoat's `launcher` feature. Annotate one async view with `#[launcher]` anywhere in the executable, including `main.rs` or a declared module. Add the export hook before opening databases or starting servers:

```rust,no_run
use topcoat::{
    Result, context::Cx, launcher,
    runtime::{Event, expr, signal},
    view::{View, view},
};

#[tokio::main]
async fn main() -> Result<()> {
    if launcher::export_if_requested().await? {
        return Ok(());
    }
    // Start your iroh server here.
    Ok(())
}

#[launcher]
async fn connection_screen(cx: &Cx) -> Result<impl View> {
    let endpoint = signal(cx, launcher::endpoint);
    let ticket = signal(cx, launcher::ticket);
    let secret = signal(cx, String::new);
    let status = signal(cx, String::new);
    let connect = launcher::connect(&expr!(endpoint.get()), &expr!(ticket.get()), &expr!(secret.get()), &status);
    Ok(view! {
        <!DOCTYPE html>
        <html>
            <head>
                <title>"Connect"</title>
                launcher::script()
            </head>
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
                    <button type="submit">"Connect"</button>
                    <p role="status">$(status.get())</p>
                </form>
            </body>
        </html>
    })
}
```

The macro uses ordinary Topcoat components and `view!`. Use signals, runtime expressions, shared components, and `asset!` as usual. `launcher::script()` loads the browser runtime and handles connection links. `launcher::connect` supplies the form's connection behavior. `launcher::endpoint()` and `launcher::ticket()` return the public defaults during export, so the example displays the configured destination immediately. To lock a supplied endpoint in your UI, add `readonly=(!launcher::endpoint().is_empty())` to its input. A read-only input is a UI choice, not access control. You own the markup and styling. App pages still use `topcoat::runtime::script()`.

The launcher registers no application route. During export, the CLI invokes the executable with `--topcoat-export-launcher OUTPUT`; the hook renders only the registered view with its own runtime and asset configuration. App routing and server startup do not run during export. The static host serves the resulting document on first visits. After connection, `/` and other app paths are served by the Rust router through the service worker. `/__topcoat/` and `/topcoat-sw.js` remain reserved for client files.

The initial Rust render runs at build time. Input bindings and event handlers run in the browser. Server procedures, shards that fetch new content, and indefinite live streams need the connected app. Values rendered or captured into launcher HTML are public. Collect client secrets from browser inputs or connection fragments instead of build-time environment variables.

# Connection Links And Identity

These URL fragment fields are reserved:

| Field | Meaning |
| --- | --- |
| `endpoint` | Server endpoint ID or serialized `EndpointAddr`. |
| `ticket` | Server endpoint ticket, supplied instead of `endpoint`. |
| `secret` | Optional client identity private key, parsed as an iroh `SecretKey`. |

Combine fields with `&` and URL-encode their values. For example, `https://app.example/__topcoat/index.html#ticket=TICKET&secret=CLIENT_KEY&tab=files`. Use the connection document at `/__topcoat/index.html` when changing servers or identities in an already connected browser. First visits can also use `/#endpoint=ENDPOINT`. Supply exactly one of `endpoint` and `ticket`. Link fields replace the configured destination; supplying both is an error. `#secret=...` alone uses the configured or previously connected endpoint or ticket. Other fragment fields remain available to the application.

Connection fields are removed from the current URL before connecting and are not carried into the application URL. A fragment is not sent in HTTP requests, but a link containing a private key still grants that identity to anyone who receives it. Prefer the form when a secret should not be shared in a link. The legacy `iroh` field remains an alias for `endpoint`.

The secret selects the client identity seen by the server through `IrohPeer`. It is not the server's private key or an application password. An empty secret creates a fresh identity. Successful settings are saved in browser storage so the worker can reconnect. Explicit identities survive worker restarts; generated identities can change after a restart. Changing an explicit identity switches its session storage. Connected renders preserve the identity authenticated by their bootstrap request. Application code remains responsible for authorizing peers and validating sessions.

# Local Preview

After building, run the server executable and serve the launcher directory:

```sh
python3 -m http.server 8080 --directory target/release/launcher
```

Open `http://localhost:8080` with a connection fragment, or use the custom form. Localhost supports service workers without HTTPS. The [iroh app example](../examples/iroh-app/src/main.rs) defines its launcher in `main.rs` beside normal pages, procedures, shards, and live updates.
