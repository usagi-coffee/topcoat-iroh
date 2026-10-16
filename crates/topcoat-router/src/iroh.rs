use std::{io, sync::Arc, time::Duration};

use futures_util::StreamExt;
use http_body::Frame;
use http_body_util::StreamBody;
use iroh::{
    EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use tokio_util::io::ReaderStream;

use crate::{Body, HeaderName, HeaderValue, Method, Router, Uri, header, request::Request};

/// The protocol used by the static browser launcher to request app resources.
pub const IROH_HTTP_ALPN: &[u8] = b"topcoat/http/1";

const MAX_HEAD_SIZE: usize = 64 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Serves a router to browsers running the static iroh launcher.
///
/// Register this handler on an iroh protocol router under [`IROH_HTTP_ALPN`].
/// Each request uses its own bidirectional stream, including its streamed
/// request and response bodies. No HTTP listener or bootstrap ticket is needed.
/// Application routes and layers still authorize every request.
///
/// This protocol carries HTTP request and response data, but does not support
/// HTTP protocol upgrades. Use the runtime's iroh transport for connected renders.
#[doc = include_str!("../docs/iroh.md")]
#[derive(Clone)]
pub struct IrohService {
    router: Arc<Router>,
    max_requests: usize,
}

impl IrohService {
    /// Creates a service allowing up to 128 concurrent requests per connection.
    #[must_use]
    pub fn new(router: Router) -> Self {
        Self {
            router: Arc::new(router),
            max_requests: 128,
        }
    }

    /// Sets the maximum number of concurrent requests per connection.
    /// Excess streams are reset before dispatching their requests.
    #[must_use]
    pub fn max_requests(mut self, max: usize) -> Self {
        self.max_requests = max;
        self
    }

    async fn request(
        router: Arc<Router>,
        peer: EndpointId,
        mut send: SendStream,
        mut receive: RecvStream,
    ) -> io::Result<()> {
        // This future owns its notification, so cancellation can interrupt a
        // handler that has not produced a response or a stalled response body.
        let stopped = send.stopped();
        let serve = async {
            let head = tokio::time::timeout(HEAD_TIMEOUT, async {
                let mut size = [0; 4];
                receive
                    .read_exact(&mut size)
                    .await
                    .map_err(io::Error::other)?;
                let size = u32::from_be_bytes(size) as usize;
                if size > MAX_HEAD_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "request headers too large",
                    ));
                }
                let mut bytes = vec![0; size];
                receive
                    .read_exact(&mut bytes)
                    .await
                    .map_err(io::Error::other)?;
                serde_json::from_slice::<RequestHead>(&bytes).map_err(io::Error::other)
            })
            .await
            .map_err(io::Error::other)??;
            let request = head.request(receive)?;
            let response = router.handle_with(request, (IrohPeer(peer),)).await;
            let head = ResponseHead {
                status: response.status().as_u16(),
                headers: response
                    .headers()
                    .iter()
                    .map(|(name, value)| {
                        Ok((name.as_str(), value.to_str().map_err(io::Error::other)?))
                    })
                    .collect::<io::Result<Vec<_>>>()?,
            };
            let head = serde_json::to_vec(&head).map_err(io::Error::other)?;
            if head.len() > MAX_HEAD_SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "response headers too large",
                ));
            }
            let size = u32::try_from(head.len()).map_err(io::Error::other)?;
            send.write_all(&size.to_be_bytes())
                .await
                .map_err(io::Error::other)?;
            send.write_all(&head).await.map_err(io::Error::other)?;
            let mut body = response.into_body().into_data_stream();
            while let Some(bytes) = body.next().await {
                send.write_all(&bytes.map_err(io::Error::other)?)
                    .await
                    .map_err(io::Error::other)?;
            }
            send.finish().map_err(io::Error::other)
        };
        tokio::select! {
            result = serve => {
                if result.is_err() {
                    let _ = send.reset(1u32.into());
                }
                result
            },
            _ = stopped => Ok(()),
        }
    }
}

impl std::fmt::Debug for IrohService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohService")
            .field("max_requests", &self.max_requests)
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for IrohService {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let mut requests = JoinSet::new();
        loop {
            let incoming = tokio::select! {
                stream = connection.accept_bi() => stream,
                _ = requests.join_next(), if !requests.is_empty() => continue,
            };
            let Ok((mut send, mut receive)) = incoming else {
                break;
            };
            while requests.try_join_next().is_some() {}
            if requests.len() >= self.max_requests {
                let _ = send.reset(2u32.into());
                let _ = receive.stop(2u32.into());
                continue;
            }
            requests.spawn(Self::request(
                Arc::clone(&self.router),
                connection.remote_id(),
                send,
                receive,
            ));
        }
        // Dropping the set aborts unfinished requests when the peer disconnects.
        Ok(())
    }
}

/// The authenticated iroh endpoint making the current request.
/// Read it with `request_context::<IrohPeer>(cx)` inside an iroh request.
/// An endpoint identity is not an application login.
#[derive(Debug, Clone, Copy)]
pub struct IrohPeer(pub EndpointId);

#[derive(Deserialize)]
struct RequestHead {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
}

impl RequestHead {
    fn request(self, receive: RecvStream) -> io::Result<Request> {
        let uri: Uri = self.url.parse().map_err(io::Error::other)?;
        if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request needs an HTTP origin",
            ));
        }
        let host = HeaderValue::from_str(uri.authority().expect("checked authority").as_str())
            .map_err(io::Error::other)?;
        let mut request = Request::new(Body::new(StreamBody::new(
            ReaderStream::new(receive).map(|chunk| chunk.map(Frame::data)),
        )));
        *request.method_mut() =
            Method::from_bytes(self.method.as_bytes()).map_err(io::Error::other)?;
        *request.uri_mut() = uri;
        for (name, value) in self.headers {
            let name = HeaderName::try_from(name).map_err(io::Error::other)?;
            // The launcher consumes decoded response bodies. It cannot act as
            // an HTTP upgrade endpoint or supply a network address.
            if matches!(
                name,
                header::HOST
                    | header::ACCEPT_ENCODING
                    | header::CONNECTION
                    | header::UPGRADE
                    | header::TRANSFER_ENCODING
                    | header::CONTENT_LENGTH
            ) {
                continue;
            }
            request.headers_mut().append(
                name,
                HeaderValue::try_from(value).map_err(io::Error::other)?,
            );
        }
        request.headers_mut().insert(header::HOST, host);
        Ok(request)
    }
}

#[derive(Serialize)]
struct ResponseHead<'a> {
    status: u16,
    headers: Vec<(&'a str, &'a str)>,
}
