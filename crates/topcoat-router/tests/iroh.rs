#![cfg(all(feature = "iroh", not(target_family = "wasm")))]

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream, presets},
};
use serde_json::{Value, json};
use topcoat::{
    Result,
    context::{Cx, app_context, request_context},
    router::{request::headers, response::Response, route},
};
use topcoat_router::{Body, IROH_HTTP_ALPN, IrohPeer, IrohService, Router};

#[route(POST "/echo")]
async fn echo(cx: &Cx, body: Body) -> Result<Response> {
    Ok(Response::builder()
        .status(201)
        .header("x-peer", request_context::<IrohPeer>(cx).0.to_string())
        .header("x-authorization", headers(cx).get("authorization").unwrap())
        .header("x-repeat", "one")
        .header("x-repeat", "two")
        .body(body)?)
}

struct Active(Arc<AtomicUsize>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[route(GET "/wait")]
async fn wait(cx: &Cx) -> Result<&'static str> {
    let count = app_context::<Arc<AtomicUsize>>(cx).clone();
    count.fetch_add(1, Ordering::SeqCst);
    let _active = Active(count);
    std::future::pending().await
}

async fn open(connection: &Connection, method: &str, path: &str) -> (SendStream, RecvStream) {
    let (mut send, receive) = connection.open_bi().await.unwrap();
    let head = json!({"method": method, "url": format!("https://app.example{path}"),
        "headers": [["origin", "https://app.example"], ["authorization", "Bearer demo"]]})
    .to_string();
    send.write_all(&u32::try_from(head.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    send.write_all(head.as_bytes()).await.unwrap();
    (send, receive)
}

async fn head(receive: &mut RecvStream) -> Value {
    let mut size = [0; 4];
    receive.read_exact(&mut size).await.unwrap();
    let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
    receive.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn active(count: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while count.load(Ordering::SeqCst) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn streams_binary_requests_and_cancels_independent_renders_without_http() {
    let server = Endpoint::bind(presets::Minimal).await.unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let router = Router::builder()
        .route(echo)
        .route(wait)
        .app_context(count.clone())
        .build();
    let listener = iroh::protocol::Router::builder(server.clone())
        .accept(IROH_HTTP_ALPN, IrohService::new(router))
        .spawn();
    let endpoint = Endpoint::bind(presets::Minimal).await.unwrap();
    let connection = endpoint
        .connect(server.addr(), IROH_HTTP_ALPN)
        .await
        .unwrap();

    let (mut pending_send, mut pending_receive) = open(&connection, "GET", "/wait").await;
    pending_send.finish().unwrap();
    active(&count, 1).await;

    let (mut send, mut receive) = open(&connection, "POST", "/echo?query=yes").await;
    let bytes: Vec<u8> = (0..=255).cycle().take(128 * 1024).collect();
    send.write_all(&bytes).await.unwrap();
    send.finish().unwrap();
    let meta = head(&mut receive).await;
    assert_eq!(meta["status"], 201);
    let headers = meta["headers"].as_array().unwrap();
    assert!(headers.contains(&json!(["x-peer", endpoint.id().to_string()])));
    assert!(headers.contains(&json!(["x-authorization", "Bearer demo"])));
    assert_eq!(
        headers
            .iter()
            .filter(|entry| entry[0] == "x-repeat")
            .count(),
        2
    );
    assert_eq!(receive.read_to_end(256 * 1024).await.unwrap(), bytes);

    pending_receive.stop(1u32.into()).unwrap();
    active(&count, 0).await;
    let (mut send, mut receive) = connection.open_bi().await.unwrap();
    send.write_all(&(64 * 1024 + 1u32).to_be_bytes())
        .await
        .unwrap();
    send.finish().unwrap();
    assert!(receive.read_to_end(1024).await.is_err());

    let (mut send, mut receive) = open(&connection, "GET", "/missing").await;
    send.finish().unwrap();
    assert_eq!(head(&mut receive).await["status"], 404);
    endpoint.close().await;
    listener.shutdown().await.unwrap();
}
