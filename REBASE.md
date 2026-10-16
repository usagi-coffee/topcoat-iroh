# Rebasing the iroh fork

This fork adds iroh transport to [Topcoat](https://github.com/tokio-rs/topcoat). Application code still uses normal Topcoat pages, procedures, shards, `live!`, and `emit!`. The [runtime guide](docs/runtime/iroh.md) covers connected renders with an HTTP entry point. The [static hosting guide](docs/router/iroh.md) covers the complete app without an HTTP listener on the Rust process.

The recorded upstream base is `b7a85c349100e99a18cd4f03c0e90dfe61a23180` (`feat: new admin panel demo (#526)`). In this checkout, `origin` points to `https://github.com/tokio-rs/topcoat`. Check remote URLs before fetching or pushing. Update this base after each successful rebase.

## Patch series

Keep the patches in the following order. Commit subjects are stable labels; their hashes change during rebases. Every fork commit starts with `iroh! <type>:`, including fixes, tests, and documentation. Keep fixes with the feature they repair when preparing the next rebase.

| Order | Commit subject after `iroh!` | Files and dependencies |
| --- | --- | --- |
| 1 | `refactor: share connected render sessions` | `crates/topcoat-runtime/src/layer/{session,socket}.rs`, `layer.rs`, and the runtime's Tokio features. Extract the shared dispatcher while retaining the WebSocket adapter. |
| 2 | `feat: add the native runtime transport` | `crates/topcoat-runtime/src/iroh_transport.rs`, its tests and guide, runtime exports, facade script selection, features, workspace dependencies, and lockfile. Builds on the shared dispatcher. |
| 3 | `feat: add browser wasm endpoint bindings` | `crates/topcoat-runtime/iroh-browser/{Cargo.toml,src/lib.rs}`, workspace membership, dependencies, lockfile, and release exclusion. Exposes the browser endpoint and runtime stream. |
| 4 | `feat: connect the browser runtime over iroh` | `crates/topcoat-runtime/browser/src/{index,runtime}.ts`, `render/{connection,iroh,iroh.test}.ts`, and regenerated `dist/index.js`. Uses the Wasm bindings and native bootstrap protocol. |
| 5 | `feat: carry session tokens in response headers` | `crates/topcoat-session/src/token{.rs,/header.rs}` and the session guide. Adds the token store needed by the static launcher. |
| 6 | `feat: serve app requests over iroh` | `crates/topcoat-router/src/iroh.rs`, exports, tests, router guides, features, and lockfile. Registers the ordinary request protocol alongside the runtime protocol. |
| 7 | `feat: stream app requests through browser wasm` | `crates/topcoat-runtime/iroh-browser/src/{lib,http}.rs`. Implements the browser half of the ordinary request protocol. |
| 8 | `feat: add the static browser launcher` | `crates/topcoat-runtime/iroh-browser/launcher/` and `scripts/build-iroh-launcher.sh`. Routes same-origin app requests through Wasm and persists endpoint/session state. Includes transport tests. |
| 9 | `feat: add runnable iroh examples` | `examples/{iroh-live,iroh-app}/`, `scripts/test-iroh-launcher.mjs`, workspace/release metadata, lockfile, `llms.txt`, and runtime/live guides. Demonstrates both deployment modes. |
| 10 | `ci: check the wasm transport and launcher` | `.github/workflows/ci.yml`. Builds the bindings and launcher and runs launcher transport tests. |
| 11 | `fix: preserve client context in connected renders` | `crates/topcoat-runtime/src/layer/session.rs`, runtime features, tests, and guide. Carries the authenticated bootstrap peer and transport into live renders, including transports supplied per request. |
| 12 | `feat: accept endpoint tickets and client identity keys` | Browser Wasm bindings, `iroh-tickets`, workspace dependencies, and lockfile. Accepts a server ticket and an optional client private key. |
| 13 | `feat: share browser connection and fragment handling` | Browser launcher files and `scripts/build-iroh-launcher.sh`. Adds reusable connection helpers, automatic bootstrap, reserved fragments, and identity-aware session storage. |
| 14 | `feat: add optional launcher views` | Router grammar/macro, shared macro paths, facade launcher API and feature, tests, guide, and lockfile. Registers a build-time Topcoat view without claiming an app route. |
| 15 | `feat: start bundled apps over iroh` | Facade startup, default iroh feature, router service context, dependencies, and lockfile. Keeps ordinary `topcoat::start` application code, loads an embedded or environment server key, checks its public identity, enforces the allowlist, and supports transport context on requests. |
| 16 | `feat: bundle apps with automatic iroh launchers` | CLI command, embedded browser source, Cargo build/metadata helpers, tests, dependencies, and lockfile. Validates separate endpoint/ticket options, selects bundled iroh startup, supplies public form defaults, and writes assets and launcher beside the Cargo executable. |
| 17 | `feat: demonstrate custom launcher connections` | `examples/iroh-app/`, browser smoke test, hosting guide, `llms.txt`, and lockfile. Exercises the custom view, automatic connection, and client identity through live renders. |
| 18 | `ci: verify launcher bundles and connection fragments` | `.github/workflows/ci.yml`. Runs the CLI bundle, fragment, and connection settings tests. |
| 19 | `docs: explain server-side apps and optional launchers` | `README.md`. Introduces the deployment model and shows a custom connection view. |

New implementation lives mostly in added files. The recurring conflicts are the small insertion points in upstream manifests, module exports, runtime startup, and documentation. Keep those edits focused rather than moving upstream code to avoid a conflict.

## Rebase procedure

Start with a clean working tree. Commit or stash unrelated work before continuing. Create a recovery branch with a fresh name, then fetch the upstream remote. Substitute its name below if `origin` points to your fork.

```sh
git status --short
git remote -v
git branch backup/iroh-before-rebase
git fetch origin
git log --oneline b7a85c349100e99a18cd4f03c0e90dfe61a23180..HEAD
git rebase --onto origin/main b7a85c349100e99a18cd4f03c0e90dfe61a23180 main
```

Use the recorded base for the old boundary and the desired upstream revision for the new boundary. Do not replay upstream commits as fork patches. If upstream has absorbed a feature, inspect its behavior and tests before dropping the matching patch. `git rebase --skip` drops the entire current patch. `git rebase --abort` restores the branch if the rebase needs to be restarted.

Resolve one patch at a time, stage its resolved files, and run `git rebase --continue`. Prefer upstream structure and adapt the small iroh insertion points to it. Do not resolve whole files with `--ours` or `--theirs` when they contain changes from both projects.

Afterward, compare the old and new series:

```sh
git range-diff b7a85c349100e99a18cd4f03c0e90dfe61a23180..backup/iroh-before-rebase origin/main..main
git diff --stat origin/main..main
git log --format='%s' origin/main..main
```

Check that only intended fork changes remain and all fork subjects use the `iroh!` prefix. After every rebase, update this file's upstream base, commands, and patch map to match the resulting history. Commit the update last, on top of the rebased series, without adding that maintenance commit to this file's patch list. Keep the recovery branch until the result has been reviewed. Rewriting local history does not require a push; coordinate before replacing a published branch.

## Conflict checks

- **Connected renders:** preserve upstream framing and rerender behavior in the shared dispatcher. WebSocket remains the default. Keep route/layer authorization, allowed run headers, concurrent render limits, and cancellation on stop, disconnect, or task shutdown. Do not let a run replace the bootstrap credentials or origin.
- **Runtime bootstrap:** keep tickets short-lived, single-use, and bound to the authenticated browser endpoint. Preserve the router's origin checks. The browser adapter and native handler must agree on `topcoat/runtime/1` and message framing.
- **Ordinary requests:** the router service and Wasm client must agree on `topcoat/http/1`, length-prefixed metadata, streaming bodies, limits, and cancellation. Preserve status codes, repeated headers, redirects, and binary bodies. Never fall back to the static host or automatically replay a failed mutation.
- **Static launcher:** reserve `/__topcoat/` and `/topcoat-sw.js`. Verify initial visits, deep links, and reconnection after the worker restarts. Hosting requires the root of a dedicated HTTPS origin; project subpaths and raw WebSocket upgrades are unsupported.
- **CLI and views:** `topcoat bundle` detects the binary markers retained by `IrohService::new` and `#[launcher]`. For a custom view it invokes `launcher::export_if_requested` before server startup. Without a registration it generates an automatic bootstrap. Native builds embed private server settings through `TOPCOAT_BUNDLE_IROH` so the fork's `topcoat::start` serves iroh; ordinary Cargo builds and the dev server keep HTTP. Keep the identity build cache private and out of the launcher and public assets. Reuse its server key and allowlist on rebundling, derive only the public endpoint for clients, and warn only when no `--iroh-secret`, `--iroh-endpoint`, or `--iroh-ticket` is supplied that cleaning the build directory removes the saved key. Public destination builds without an explicit key must avoid the private cache and require a matching `TOPCOAT_IROH_SECRET` at runtime. Reject mismatched explicit build keys before building, including ticket endpoints. Expose only public endpoint/ticket defaults to custom launcher views. Output belongs beside the Cargo executable (`target/release/launcher` by default), and publishing must preserve Cargo's other files. Keep both markers and the CLI scanner aligned, including release builds. Launcher discovery is independent of page discovery and must not claim `/`. Keep `crates/topcoat/src/launcher.rs` and the browser connection API aligned. The CLI embeds the browser source and resolves its standalone manifest in `build.rs`; installing it must not leave a runtime dependency on the source checkout. Keep standalone Wasm build outputs isolated by source version so an old cdylib cannot be packaged with new JavaScript. Rebuild an external package with and without a launcher after changes to Cargo metadata, asset bundling, or view rendering.
- **Client identity:** endpoint tickets and explicit client secrets are accepted by the browser bindings. Keep connection settings atomic and sessions separate when a supplied client identity changes. Preserve the bootstrap request's `IrohPeer` in connected renders. Verify it after reconnecting and restarting the worker. Keep server `--iroh-secret` separate from client `#secret`. Enforce `--iroh-peer` before app requests and live bootstrap, and require a bound ticket for runtime connections. Keep separate endpoint/ticket config fields and `endpoint`, `ticket`, and `secret` fragment handling aligned across clients, consume reserved fields before navigation, and preserve application fragment fields.
- **Sessions:** synthetic responses cannot set browser cookies. Keep `HeaderTokenStore` and the launcher's bearer/session-header handling aligned, including renewal and logout before redirect navigation. Browser storage has no `HttpOnly` protection. Application storage still validates session records and expiry.
- **Cargo:** merge workspace members, dependencies, features, and release exclusions. Resolve manifests first, then regenerate the lockfile with Cargo while retaining upstream dependency choices where compatible. Do not copy an old lockfile wholesale over an upstream update. Native-only iroh modules must remain gated out of browser builds.
- **Generated files:** regenerate the browser bundles from resolved TypeScript. Do not hand-merge minified JavaScript. Keep the linked `wasm-bindgen-cli-support` version, the standalone build script's CLI version, and `Cargo.lock` aligned; update pinned versions in CI and guides together. Publish matching JavaScript and Wasm files.
- **Documentation:** retain upstream API guidance in `llms.txt` and add the fork's transport notes. Crate-local guide files are relative symlinks into `docs/`. Keep the README focused on server-owned state, familiar Topcoat APIs, deployment, and the optional launcher, with a link here for rebase details.

## Verification

Follow [.agents/skills/check/SKILL.md](.agents/skills/check/SKILL.md) for the full workspace and browser checks, including nightly formatting, macro formatting, Clippy, tests, and rustdoc. Run the runtime browser install/lint/build/test commands when resolving its TypeScript and include regenerated bundles in that patch. Default WebSocket tests and runtime coherence tests must still pass.

Run the iroh checks as well:

```sh
cargo test -p topcoat-runtime --features iroh --test iroh --locked
cargo test -p topcoat-router --features iroh --test iroh --locked
cargo clippy -p topcoat-runtime-iroh-browser --target wasm32-unknown-unknown --locked -- -D warnings
node crates/topcoat-runtime/iroh-browser/launcher/transport.test.js
node crates/topcoat-runtime/iroh-browser/launcher/fragment.test.js
node crates/topcoat-runtime/iroh-browser/launcher/settings.test.js
bash scripts/build-iroh-launcher.sh
cargo topcoat bundle -p iroh-app --profile dev
```

Use the [example and browser smoke test](docs/router/iroh.md#example) with a fresh Chromium profile. It must load app HTML and assets, run procedures and shards, receive live updates, upload binary data, navigate, and retain sessions after restarting the service worker. The Rust example must have no HTTP listener; only the launcher files come from the static server. Test relay connectivity from separate networks before a release, since a local native endpoint test does not establish that deployment works across networks.
