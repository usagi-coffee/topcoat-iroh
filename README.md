# Topcoat with iroh

A fork of [Topcoat](https://github.com/tokio-rs/topcoat) with iroh integration. See [REBASE.md](REBASE.md) for integration details and the rebase workflow.

## Differences

With the static iroh launcher, you still write normal Topcoat pages, components, procedures, shards, `live!`, and `emit!`. Deployment and server startup change:

| Area | Standard HTTP deployment | Static iroh deployment |
| --- | --- | --- |
| Domain | Points to the app's HTTP server or reverse proxy. | Points to a static host serving the launcher and its JavaScript/Wasm files. |
| Rust server | Needs an HTTP endpoint reachable by visitors. | Needs no public HTTP port, inbound port forwarding, VPN, or configured tunnel. Keep the app running with access to iroh relays. |
| Build and deploy | Deploy the Rust app and its assets. | Also build and publish the launcher. Pages, app assets, requests, and live updates come from the Rust app through iroh. |
| Startup | Start the Topcoat HTTP server. | Register the router with iroh and configure the launcher with its public endpoint ID. Persist the endpoint's secret key to keep that address across restarts. |
| Sessions | Use the usual cookie-based session store. | Use `HeaderTokenStore`. Tokens live in browser storage without `HttpOnly` protection; cookie-based authentication needs adaptation. |

Host the launcher at the root of a dedicated HTTPS origin, such as a custom domain or GitHub Pages user site. Project subpaths are not supported. Custom WebSocket routes are also unsupported; Topcoat live updates use iroh.

See the [static hosting guide](docs/router/iroh.md) for setup. These differences apply to the full launcher deployment; enabling only `runtime-iroh` still requires an HTTP entry point.
