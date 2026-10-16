#![doc = include_str!("../docs/launcher.md")]

use std::{fs, path::Path};

use crate::{
    Error, Result,
    asset::{AssetBundle, AssetConfig, RouterBuilderAssetExt},
    router::{Body, Method, PageFn, PageRenderFn, Router, request::Request, to_bytes},
    runtime::{Event, Expr, Js, RouterBuilderRuntimeExt, Signal},
};

/// The build-time view registered by `#[launcher]`.
#[doc(hidden)]
pub struct Registration {
    pub marker: &'static [u8],
    pub name: &'static str,
    pub render: PageRenderFn,
}

inventory::collect!(Registration);

/// The public server endpoint supplied during launcher export, or an empty string.
///
/// Use `signal(cx, launcher::endpoint)` to prefill a connection field. This is
/// empty when the bundle uses a ticket instead. It never contains the server key.
#[must_use]
pub fn endpoint() -> String {
    std::env::var("TOPCOAT_LAUNCHER_ENDPOINT").unwrap_or_default()
}

/// The public server ticket supplied during launcher export, or an empty string.
///
/// Use `signal(cx, launcher::ticket)` to prefill a connection field. This is
/// empty when the bundle uses an endpoint instead.
#[must_use]
pub fn ticket() -> String {
    std::env::var("TOPCOAT_LAUNCHER_TICKET").unwrap_or_default()
}

/// Exports the registered launcher when invoked by `topcoat bundle`.
///
/// Call this before starting servers or opening application resources. Return
/// from `main` when it returns `true`. Ordinary invocations return `false`.
/// Apps without a custom launcher do not need this hook or the `launcher` feature.
///
/// # Errors
///
/// Returns an error for invalid export arguments, missing or multiple launcher
/// registrations, missing assets, a failed render, or an unwritable output.
pub async fn export_if_requested() -> Result<bool> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--topcoat-export-launcher")) {
        return Ok(false);
    }
    let out = args
        .next()
        .ok_or_else(|| Error::msg("missing launcher output directory"))?;
    if args.next().is_some() {
        return Err(Error::msg(
            "unexpected arguments after launcher output directory",
        ));
    }
    export(Path::new(&out)).await?;
    Ok(true)
}

async fn export(out: &Path) -> Result<()> {
    let mut registrations = inventory::iter::<Registration>.into_iter();
    let registration = registrations
        .next()
        .ok_or_else(|| Error::msg("no #[launcher] view registered"))?;
    // Keep the macro's marker in optimized executables for CLI discovery.
    std::hint::black_box(registration.marker);
    if let Some(other) = registrations.next() {
        return Err(Error::msg(format!(
            "only one #[launcher] view is allowed per executable: {} and {}",
            registration.name, other.name,
        )));
    }
    let assets = AssetBundle::load_dir(out.join("__topcoat/assets"))?;
    // This router exists only during export. It does not discover application routes.
    let router = Router::builder()
        .page(PageFn::new(
            Method::GET,
            "/__topcoat/launcher",
            registration.render,
        ))
        .assets(AssetConfig::hosted_at(
            "/__topcoat/assets",
            assets.catalog().clone(),
        ))
        .runtime()
        .build();
    let response = router
        .handle(
            Request::builder()
                .uri("/__topcoat/launcher")
                .body(Body::empty())?,
        )
        .await;
    if response.status() != 200 {
        return Err(Error::msg(format!(
            "launcher view returned {}",
            response.status()
        )));
    }
    let html = to_bytes(response.into_body(), 16 * 1024 * 1024).await?;
    for path in ["index.html", "404.html", "__topcoat/index.html"] {
        fs::write(out.join(path), &html)?;
    }
    Ok(())
}

/// Loads the browser runtime and handles reserved connection fragments.
///
/// Include this in the head of a custom launcher. A link containing `endpoint`
/// or `ticket` connects automatically, with `secret` selecting the client identity.
#[crate::view::component]
pub async fn script() -> Result<impl crate::view::View> {
    Ok(crate::view::view! {
        crate::runtime::script()
        <script
            type="module"
            src="/__topcoat/bootstrap.js"
            data-topcoat-launcher="custom"
        ></script>
    })
}

/// A form submit handler that connects to an app and navigates to it.
///
/// Bind inputs to ordinary signals, pass their expressions here, and render
/// `status` to display connection progress or an error. Supply either an endpoint
/// or a ticket, leaving the other expression empty. Leave both empty to use
/// the bundled server. An empty secret creates
/// a fresh browser identity. The secret is a client private key, not a server key.
///
/// # Panics
///
/// Invoking the returned handler on the server panics. It runs only in the browser.
///
/// ```rust
/// # use topcoat::{Result, context::Cx, launcher, runtime::{Event, expr, signal}, view::*};
/// # #[component]
/// # async fn example(cx: &Cx) -> Result<impl View> {
/// let endpoint = signal(cx, launcher::endpoint);
/// let ticket = signal(cx, launcher::ticket);
/// let secret = signal(cx, String::new);
/// let status = signal(cx, String::new);
/// let connect = launcher::connect(&expr!(endpoint.get()), &expr!(ticket.get()), &expr!(secret.get()), &status);
/// # Ok(view! {
/// <form @submit=(connect)>
///     <input :value=$(endpoint.get()) @input=$(|e: Event| endpoint.set(e.target.value))>
///     <input :value=$(ticket.get()) @input=$(|e: Event| ticket.set(e.target.value))>
///     <input type="password" :value=$(secret.get()) @input=$(|e: Event| secret.set(e.target.value))>
///     <button type="submit">"Connect"</button>
///     <p role="status">$(status.get())</p>
/// </form>
/// # })
/// # }
/// ```
#[must_use]
pub fn connect(
    endpoint: &Expr<String>,
    ticket: &Expr<String>,
    secret: &Expr<String>,
    status: &Signal<String>,
) -> Expr<impl FnOnce(Event) + use<>> {
    let js = Js::builder()
        .source("async (event) => { event.prevent_default(); const status = ")
        .expression(&Expr::from(status.clone()))
        .source("; status.set(cx.hydrate('Connecting...')); try { const launcher = await import('/__topcoat/connect.js'); await launcher.connect({ endpoint: String(")
        .expression(endpoint)
        .source("), ticket: String(")
        .expression(ticket)
        .source("), secret: String(")
        .expression(secret)
        .source(") }); launcher.openApp(); } catch (error) { status.set(cx.hydrate(String(error))); } }")
        .build();
    Expr::evaluate(
        || |_event: Event| panic!("launcher connection runs in the browser"),
        js,
    )
}

#[cfg(all(test, feature = "discover"))]
mod tests {
    use super::*;
    mod arbitrary_module {
        use crate::{
            Result,
            context::Cx,
            launcher,
            runtime::signal,
            view::{View, view},
        };

        #[launcher]
        async fn connection_screen(cx: &Cx) -> Result<impl View> {
            let label = signal(cx, || "<connect>".to_owned());
            Ok(view! { <p>$(label.get())</p> })
        }
    }

    #[tokio::test]
    async fn exports_a_topcoat_view_from_an_arbitrary_module() {
        let out = tempfile::tempdir().unwrap();
        fs::create_dir_all(out.path().join("__topcoat/assets")).unwrap();
        crate::asset::Manifest {
            version: crate::asset::MANIFEST_VERSION,
            assets: Vec::new(),
        }
        .save(out.path().join("__topcoat/assets/manifest.toml"))
        .unwrap();
        export(out.path()).await.unwrap();
        let html = fs::read_to_string(out.path().join("index.html")).unwrap();
        assert!(html.contains("&lt;connect&gt;"));
        assert!(!html.contains("Application root"));
        assert_eq!(
            html,
            fs::read_to_string(out.path().join("404.html")).unwrap()
        );
    }
}
