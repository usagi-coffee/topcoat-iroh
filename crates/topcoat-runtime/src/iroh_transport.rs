use std::{
    collections::HashMap,
    fmt, io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use iroh::{
    Endpoint, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};
use topcoat_core::{
    context::{Cx, try_app_context, try_request_context},
    error::Result,
};
use topcoat_router::{
    Body, Method, StatusCode,
    content::Json,
    header,
    request::{FromRequest, headers, method},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::{
    MaxRunsPerConnection,
    layer::session::{self, DEFAULT_MAX_RUNS_PER_CONNECTION, RenderConnection},
};

/// The versioned iroh protocol for connected page and shard renders.
pub const IROH_RUNTIME_ALPN: &[u8] = b"topcoat/runtime/1";

const TICKET_LIFETIME: Duration = Duration::from_secs(30);
const MAX_TICKETS: usize = 1024;
const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;

/// Delivers connected renders through an iroh endpoint.
///
/// Enable the facade's `runtime-iroh` feature. Register a clone in the
/// Topcoat router's app context and register this handler on an iroh
/// protocol router with [`IROH_RUNTIME_ALPN`]. `runtime::script()` reads
/// the browser module URL from this value.
///
/// The browser first obtains a single-use ticket over HTTP using its
/// cookies. Tickets expire after 30 seconds and are bound to the browser's
/// iroh public key. Each render inherits the HTTP handshake's credentials
/// and passes through the application's routes and layers.
#[doc = include_str!("../docs/iroh.md")]
#[derive(Clone)]
pub struct IrohTransport {
    endpoint: Endpoint,
    browser_module: String,
    tickets: Arc<Mutex<HashMap<Uuid, Ticket>>>,
}

impl IrohTransport {
    /// The transport supplied by iroh startup or registered on the router.
    #[doc(hidden)]
    #[must_use]
    pub fn from_context(cx: &Cx) -> Option<&Self> {
        try_request_context(cx).or_else(|| try_app_context(cx))
    }

    /// Creates a transport using `endpoint` and the URL of the generated
    /// `topcoat_runtime_iroh_browser.js` module. Serve its companion Wasm
    /// file beside it. The module is loaded only when a live render connects.
    #[must_use]
    pub fn new(endpoint: Endpoint, browser_module: impl Into<String>) -> Self {
        Self {
            endpoint,
            browser_module: browser_module.into(),
            tickets: Arc::default(),
        }
    }

    /// The URL of the browser's Wasm binding module.
    #[must_use]
    pub fn browser_module(&self) -> &str {
        &self.browser_module
    }

    pub(crate) fn requested(cx: &Cx) -> bool {
        *method(cx) == Method::POST
            && headers(cx)
                .get("x-topcoat-iroh")
                .is_some_and(|value| value == "true")
    }

    pub(crate) async fn bootstrap(cx: &Cx, body: Body) -> Result<Response> {
        let Some(transport) = Self::from_context(cx) else {
            return StatusCode::NOT_FOUND.into_response(cx);
        };
        let Json(peer) = Json::<EndpointId>::from_request(cx, body).await?;
        let now = Instant::now();
        let mut tickets = transport.tickets.lock().expect("ticket lock poisoned");
        tickets.retain(|_, ticket| now.duration_since(ticket.created) < TICKET_LIFETIME);
        if tickets.len() >= MAX_TICKETS {
            return StatusCode::TOO_MANY_REQUESTS.into_response(cx);
        }
        let ticket = Uuid::new_v4();
        tickets.insert(
            ticket,
            Ticket {
                created: now,
                peer,
                connection: Arc::new(RenderConnection::from_handshake(cx)),
                max_runs: try_app_context::<MaxRunsPerConnection>(cx)
                    .map_or(DEFAULT_MAX_RUNS_PER_CONNECTION, |max| max.0),
            },
        );
        let pending = Arc::downgrade(&transport.tickets);
        tokio::spawn(async move {
            tokio::time::sleep(TICKET_LIFETIME).await;
            if let Some(pending) = pending.upgrade() {
                pending
                    .lock()
                    .expect("ticket lock poisoned")
                    .remove(&ticket);
            }
        });
        let mut response = Json(Bootstrap {
            ticket,
            address: transport.endpoint.addr(),
        })
        .into_response(cx)?;
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            topcoat_router::HeaderValue::from_static("no-store"),
        );
        Ok(response)
    }

    fn redeem(&self, id: Uuid, peer: EndpointId) -> io::Result<Ticket> {
        let mut tickets = self.tickets.lock().expect("ticket lock poisoned");
        if let Some(ticket) = tickets.get(&id)
            && ticket.peer == peer
            && ticket.created.elapsed() < TICKET_LIFETIME
        {
            return Ok(tickets.remove(&id).expect("ticket exists"));
        }
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid transport ticket",
        ))
    }

    async fn serve(&self, connection: &Connection) -> std::result::Result<(), AcceptError> {
        let (ticket, send, receive) = tokio::time::timeout(TICKET_LIFETIME, async {
            let (send, receive) = connection.accept_bi().await?;
            let codec = LengthDelimitedCodec::builder()
                .max_frame_length(1024)
                .new_codec();
            let mut receive = FramedRead::new(receive, codec);
            let first = receive
                .next()
                .await
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))??;
            let hello: Hello = serde_json::from_slice(&first).map_err(io::Error::other)?;
            let ticket = self.redeem(hello.ticket, connection.remote_id())?;
            receive.decoder_mut().set_max_frame_length(MAX_MESSAGE_SIZE);
            Ok::<_, AcceptError>((ticket, send, receive))
        })
        .await
        .map_err(|error| io::Error::new(io::ErrorKind::TimedOut, error))??;
        let codec = LengthDelimitedCodec::builder()
            .max_frame_length(MAX_MESSAGE_SIZE)
            .new_codec();
        let send = FramedWrite::new(send, codec)
            .with(|text: String| std::future::ready(Ok::<_, io::Error>(Bytes::from(text))));
        let receive = receive.map(|frame| {
            frame.and_then(|bytes| String::from_utf8(bytes.to_vec()).map_err(io::Error::other))
        });
        session::run(ticket.connection, send, receive, ticket.max_runs).await;
        Ok(())
    }
}

impl fmt::Debug for IrohTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IrohTransport")
            .field("endpoint", &self.endpoint.id())
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for IrohTransport {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let result = self.serve(&connection).await;
        connection.close(0u32.into(), b"runtime session ended");
        result
    }
}

struct Ticket {
    created: Instant,
    peer: EndpointId,
    connection: Arc<RenderConnection>,
    max_runs: usize,
}

#[derive(Serialize)]
struct Bootstrap {
    ticket: Uuid,
    address: iroh::EndpointAddr,
}

#[derive(Deserialize)]
struct Hello {
    ticket: Uuid,
}

#[cfg(test)]
mod tests {
    use iroh::endpoint::presets;
    use topcoat_router::{Router, request::Request, to_bytes};

    use super::*;
    use crate::RouterBuilderRuntimeExt;

    #[tokio::test]
    async fn expired_tickets_cannot_start_a_session() {
        let endpoint = Endpoint::bind(presets::Minimal).await.unwrap();
        let transport = IrohTransport::new(endpoint.clone(), "/iroh/browser.js");
        let router = Router::builder()
            .runtime()
            .app_context(transport.clone())
            .build();
        let response = router
            .handle(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header("x-topcoat-iroh", "true")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&endpoint.id()).unwrap()))
                    .unwrap(),
            )
            .await;
        let bootstrap: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id = serde_json::from_value(bootstrap["ticket"].clone()).unwrap();
        transport
            .tickets
            .lock()
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .created = Instant::now().checked_sub(TICKET_LIFETIME).unwrap();
        assert_eq!(
            transport.redeem(id, endpoint.id()).err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        // Release the router retained by the deliberately expired test ticket.
        transport.tickets.lock().unwrap().clear();
        endpoint.close().await;
    }
}
