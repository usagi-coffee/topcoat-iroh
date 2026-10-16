#![cfg(all(feature = "iroh", not(target_family = "wasm")))]

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream, presets},
};
use serde_json::{Value, json};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        Body, Router,
        error::forbidden,
        page,
        request::{Request, headers},
        to_bytes,
    },
    runtime::{connected, shard},
    view::{View, emit, live},
};
use topcoat_runtime::{IROH_RUNTIME_ALPN, IrohTransport, RouterBuilderRuntimeExt};

struct Active(Arc<AtomicUsize>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[page("/room")]
async fn room(cx: &Cx) -> Result<impl View> {
    if headers(cx)
        .get("cookie")
        .is_none_or(|cookie| cookie != "session=allowed")
    {
        return Err(forbidden().into());
    }
    let active = app_context::<Arc<AtomicUsize>>(cx).clone();
    Ok(live! {
        active.fetch_add(1, Ordering::SeqCst);
        let _guard = Active(active);
        let token = emit! {
            <p>
                "connected="
                (connected(cx))
            </p>
        }?;
        if connected(cx) {
            tokio::task::yield_now().await;
            emit! { <p>"updated"</p> }?;
            std::future::pending::<()>().await;
        }
        Ok(token)
    })
}

#[shard("/feed")]
async fn feed(value: String) -> Result<impl View> {
    Ok(live! {
        let token = emit! { <p>(value)</p> }?;
        std::future::pending::<()>().await;
        Ok(token)
    })
}

struct Client {
    connection: Connection,
    send: FramedWrite<SendStream, LengthDelimitedCodec>,
    receive: FramedRead<RecvStream, LengthDelimitedCodec>,
}

impl Client {
    async fn connect(endpoint: &Endpoint, address: iroh::EndpointAddr, ticket: &Value) -> Self {
        let connection = endpoint.connect(address, IROH_RUNTIME_ALPN).await.unwrap();
        let (send, receive) = connection.open_bi().await.unwrap();
        let mut client = Self {
            connection,
            send: FramedWrite::new(send, LengthDelimitedCodec::new()),
            receive: FramedRead::new(receive, LengthDelimitedCodec::new()),
        };
        client.send(json!({"ticket": ticket})).await;
        client
    }

    async fn send(&mut self, value: Value) {
        self.send
            .send(Bytes::from(value.to_string()))
            .await
            .unwrap();
    }

    async fn frame(&mut self) -> Value {
        let bytes = tokio::time::timeout(Duration::from_secs(5), self.receive.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn run(&mut self, id: u64) {
        self.send(json!({
            "run": id, "method": "POST", "path": "/room",
            "headers": {"content-type": "application/json", "x-topcoat-runtime": "true"},
            "body": "{}"
        }))
        .await;
    }

    async fn snapshot_and_swap(&mut self, id: u64) {
        let first = self.frame().await;
        assert_eq!(first["run"], id);
        assert_eq!(first["frame"]["t"], "snapshot");
        assert!(
            first["frame"]["html"]
                .as_str()
                .unwrap()
                .contains("connected=true")
        );
        let swap = self.frame().await;
        assert_eq!(swap["run"], id);
        assert_eq!(swap["frame"]["t"], "swap");
        assert!(swap["frame"]["html"].as_str().unwrap().contains("updated"));
    }
}

async fn bootstrap(
    router: &Router,
    peer: &Endpoint,
    origin: &str,
) -> topcoat::router::response::Response {
    router
        .handle(
            Request::builder()
                .method("POST")
                .uri("/room")
                .header("host", "app.example")
                .header("origin", origin)
                .header("cookie", "session=allowed")
                .header("content-type", "application/json")
                .header("x-topcoat-iroh", "true")
                .body(Body::from(json!(peer.id()).to_string()))
                .unwrap(),
        )
        .await
}

async fn ticket(router: &Router, peer: &Endpoint) -> Value {
    let response = bootstrap(router, peer, "https://app.example").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap()
}

async fn wait_active(active: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while active.load(Ordering::SeqCst) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn iroh_renders_pages_and_shards_and_cancels_runs() {
    let server = Endpoint::bind(presets::Minimal).await.unwrap();
    let transport = IrohTransport::new(server.clone(), "/iroh/browser.js");
    let listener = iroh::protocol::Router::builder(server.clone())
        .accept(IROH_RUNTIME_ALPN, transport.clone())
        .spawn();
    let active = Arc::new(AtomicUsize::new(0));
    let router = Router::builder()
        .page(room)
        .route(feed)
        .runtime()
        .max_runs_per_connection(2)
        .app_context(transport)
        .app_context(active.clone())
        .build();
    let peer = Endpoint::bind(presets::Minimal).await.unwrap();
    let bootstrap = ticket(&router, &peer).await;
    let mut client = Client::connect(&peer, server.addr(), &bootstrap["ticket"]).await;

    client.run(1).await;
    client.snapshot_and_swap(1).await;
    client
        .send(json!({"run": 2, "method": "POST", "path": "/feed",
            "headers": {"content-type": "application/json"},
            "body": json!({"args": ["news"], "signals": {}}).to_string()
        }))
        .await;
    let shard = client.frame().await;
    assert_eq!(shard["run"], 2);
    assert!(shard["frame"]["html"].as_str().unwrap().contains("news"));
    client.run(3).await;
    assert_eq!(
        client.frame().await,
        json!({"run":3,"frame":{"t":"error","status":429}})
    );
    client.send(json!({"stop":1})).await;
    wait_active(&active, 0).await;
    client
        .send(json!({"run":4,"method":"GET","path":"/room","headers":{"cookie":"session=forged"}}))
        .await;
    assert_eq!(client.frame().await["frame"]["status"], 400);
    client.run(5).await;
    client.snapshot_and_swap(5).await;
    client.connection.close(0u32.into(), b"disconnect");
    wait_active(&active, 0).await;

    let fresh = ticket(&router, &peer).await;
    let mut client = Client::connect(&peer, server.addr(), &fresh["ticket"]).await;
    client.run(6).await;
    client.snapshot_and_swap(6).await;
    // Aborting the handler on shutdown also drops its render tasks.
    listener.shutdown().await.unwrap();
    wait_active(&active, 0).await;
    peer.close().await;
}

#[tokio::test]
async fn tickets_require_the_http_origin_and_bound_peer_and_cannot_be_replayed() {
    let server = Endpoint::bind(presets::Minimal).await.unwrap();
    let transport = IrohTransport::new(server.clone(), "/iroh/browser.js");
    let listener = iroh::protocol::Router::builder(server.clone())
        .accept(IROH_RUNTIME_ALPN, transport.clone())
        .spawn();
    let router = Router::builder().runtime().app_context(transport).build();
    let peer = Endpoint::bind(presets::Minimal).await.unwrap();
    let stranger = Endpoint::bind(presets::Minimal).await.unwrap();
    assert_eq!(
        bootstrap(&router, &peer, "https://evil.example")
            .await
            .status(),
        403
    );
    let bootstrap = ticket(&router, &peer).await;
    let wrong = Client::connect(&stranger, server.addr(), &bootstrap["ticket"]).await;
    tokio::time::timeout(Duration::from_secs(5), wrong.connection.closed())
        .await
        .unwrap();
    let mut right = Client::connect(&peer, server.addr(), &bootstrap["ticket"]).await;
    right.run(1).await;
    assert_eq!(right.frame().await["frame"]["status"], 404);
    let replay = Client::connect(&peer, server.addr(), &bootstrap["ticket"]).await;
    tokio::time::timeout(Duration::from_secs(5), replay.connection.closed())
        .await
        .unwrap();
    listener.shutdown().await.unwrap();
    peer.close().await;
    stranger.close().await;
}
