use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime},
};

use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt, asset},
    context::{Cx, app_context, try_request_context},
    launcher,
    router::{
        Body, IrohPeer, Router, RouterBuilderDiscoverExt,
        error::see_other,
        href, page,
        request::Bytes,
        response::{IntoResponse, Response},
        route,
    },
    runtime::{Event, RouterBuilderRuntimeExt, connected, expr, procedure, shard, signal},
    session::{self, HeaderTokenStore, RouterBuilderSessionExt, SessionConfig, TokenHash},
    view::{View, emit, live, view},
};

#[tokio::main]
async fn main() -> Result<()> {
    if launcher::export_if_requested().await? {
        return Ok(());
    }
    let router = Router::builder()
        .discover()
        .assets(AssetBundle::load()?)
        .sessions(
            SessionConfig::builder()
                .token_store(HeaderTokenStore)
                .build(),
        )
        .app_context(Sessions::default())
        .runtime()
        .build();
    topcoat::start(router).await?;
    Ok(())
}

#[derive(Default)]
struct Sessions(Mutex<HashMap<TokenHash, SystemTime>>);

#[page("/")]
async fn home(cx: &Cx) -> Result<impl View> {
    let peer = try_request_context::<IrohPeer>(cx).map(|peer| peer.0.to_string());
    let count = signal(cx, || 2usize);
    let name = signal(cx, || "world".to_owned());
    let active = if let Some(hash) = session::token_hash(cx).await? {
        app_context::<Sessions>(cx)
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&hash)
            .is_some_and(|expiry| *expiry > SystemTime::now())
    } else {
        false
    };
    Ok(view! {
        <!DOCTYPE html>
        <html>
            <head>
                <title>"App over iroh"</title>
                <link rel="stylesheet" href=(asset!("./style.css"))>
                topcoat::runtime::script()
            </head>
            <body>
                <h1>"App over iroh"</h1>
                <p>"This page and its assets arrived over iroh."</p>
                <p id="peer">(peer.unwrap_or_default())</p>
                <p id="session">
                    "Session: "
                    if active {
                        "active"
                    } else {
                        "none"
                    }
                </p>
                <form method="POST" action=(href!(login))>
                    <button>"Start demo session"</button>
                </form>
                <form method="POST" action=(href!(logout))>
                    <button>"End demo session"</button>
                </form>
                <p><a href=(href!(next))>"Open another page"</a></p>
                <button
                    id="double"
                    @click=$(async |_e| count.set(double(count.get()).await))
                >
                    "Double"
                </button>
                <p id="value">$(count.get())</p>
                <input
                    id="name"
                    :value=$(name.get())
                    @input=$(|e: Event| name.set(e.target.value))
                >
                greeting(name: $(name.get()))
                (live! {
                    let mut tick = 0usize;
                    loop {
                        let token = emit! {
                            <p id="tick">
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

#[page("/next")]
async fn next() -> Result<impl View> {
    Ok(view! {
        <h1>"Another page over iroh"</h1>
        <a href=(href!(home))>"Back"</a>
    })
}

#[procedure]
async fn double(value: usize) -> Result<usize> {
    Ok(value.saturating_mul(2))
}

#[shard]
async fn greeting(name: String) -> Result<impl View> {
    Ok(view! {
        <p id="greeting">
            "Hello, "
            (name)
        </p>
    })
}

#[route(POST "/login")]
async fn login(cx: &Cx) -> Result<Response> {
    let session = session::start(cx).await?;
    app_context::<Sessions>(cx)
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(session.token_hash, session.expires_at);
    see_other(href!(home).resolve(cx)).into_response(cx)
}

#[route(POST "/logout")]
async fn logout(cx: &Cx) -> Result<Response> {
    if let Some(hash) = session::stop(cx).await? {
        app_context::<Sessions>(cx)
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&hash);
    }
    see_other(href!(home).resolve(cx)).into_response(cx)
}

#[route(POST "/echo")]
async fn echo(bytes: Bytes) -> Result<Body> {
    Ok(Body::from(bytes))
}

#[launcher]
async fn connection_screen(cx: &Cx) -> Result<impl View> {
    let endpoint = signal(cx, launcher::endpoint);
    let ticket = signal(cx, launcher::ticket);
    let secret = signal(cx, String::new);
    let status = signal(cx, String::new);
    let connect = launcher::connect(
        &expr!(endpoint.get()),
        &expr!(ticket.get()),
        &expr!(secret.get()),
        &status,
    );
    Ok(view! {
        <!DOCTYPE html>
        <html>
            <head>
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Connect to app"</title>
                <link rel="stylesheet" href=(asset!("./style.css"))>
                launcher::script()
            </head>
            <body>
                <h1>"Connect to app"</h1>
                <form @submit=(connect)>
                    <label>
                        "Server endpoint"
                        <input
                            id="endpoint"
                            autocomplete="off"
                            :value=$(endpoint.get())
                            @input=$(|e: Event| endpoint.set(e.target.value))
                        >
                    </label>
                    <label>
                        "Server ticket (instead of endpoint)"
                        <input
                            id="ticket"
                            autocomplete="off"
                            :value=$(ticket.get())
                            @input=$(|e: Event| ticket.set(e.target.value))
                        >
                    </label>
                    <label>
                        "Client secret (optional)"
                        <input
                            id="secret"
                            type="password"
                            autocomplete="off"
                            :value=$(secret.get())
                            @input=$(|e: Event| secret.set(e.target.value))
                        >
                    </label>
                    <button type="submit">"Connect"</button>
                    <p role="status">$(status.get())</p>
                </form>
            </body>
        </html>
    })
}
