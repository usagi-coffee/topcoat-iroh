use std::time::Duration;

use iroh::{Endpoint, endpoint::presets};
use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt},
    context::Cx,
    router::{RouterBuilderDirectoryExt, module_router, page},
    runtime::{IROH_RUNTIME_ALPN, IrohTransport, RouterBuilderRuntimeExt, connected},
    view::{View, emit, live, view},
};

#[tokio::main]
async fn main() -> Result<()> {
    let endpoint = Endpoint::bind(presets::N0).await?;
    let transport = IrohTransport::new(endpoint.clone(), "/iroh/topcoat_runtime_iroh_browser.js");
    let listener = iroh::protocol::Router::builder(endpoint)
        .accept(IROH_RUNTIME_ALPN, transport.clone())
        .spawn();
    let router = module_router!()
        .assets(AssetBundle::load()?)
        .serve_dir(
            "/iroh/{*rest}",
            concat!(env!("CARGO_MANIFEST_DIR"), "/public/iroh"),
        )
        .app_context(transport)
        .runtime()
        .build();
    let result = topcoat::start(router).await;
    listener.shutdown().await?;
    Ok(result?)
}

#[page]
async fn home(cx: &Cx) -> Result<impl View> {
    Ok(view! {
        <!DOCTYPE html>
        <html>
            <head>
                <title>"Iroh live updates"</title>
                topcoat::runtime::script()
            </head>
            <body>
                <h1>"Iroh live updates"</h1>
                (live! {
                    let mut tick = 0;
                    loop {
                        let token = emit! {
                            <p>
                                "Tick: "
                                (tick)
                            </p>
                        }?;
                        if !connected(cx) {
                            break Ok(token);
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        tick += 1;
                    }
                })
            </body>
        </html>
    })
}
