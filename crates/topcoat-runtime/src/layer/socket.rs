//! Renders pages and shards over one WebSocket per document.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use topcoat_core::{
    context::{Cx, try_app_context},
    error::Result,
};
use topcoat_router::{
    Body, HeaderMap, Method,
    content::websocket::{Message, WebSocketUpgrade},
    header,
    request::{FromRequest, headers, method},
    response::Response,
};

use super::{
    MaxRunsPerConnection,
    session::{self, DEFAULT_MAX_RUNS_PER_CONNECTION, RenderConnection},
};
use crate::RUNTIME_PROTOCOL;

/// Checks for a `GET` that requests the runtime WebSocket subprotocol.
pub(super) fn requested(cx: &Cx) -> bool {
    *method(cx) == Method::GET && requests_runtime_protocol(headers(cx))
}

/// Checks whether the headers request the runtime subprotocol.
fn requests_runtime_protocol(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|protocol| protocol.trim() == RUNTIME_PROTOCOL)
}

/// Opens the WebSocket and starts handling render requests.
pub(super) async fn accept(cx: &Cx, body: Body) -> Result<Response> {
    let upgrade = WebSocketUpgrade::from_request(cx, body).await?;
    let connection = Arc::new(RenderConnection::from_handshake(cx));
    let max_runs = try_app_context::<MaxRunsPerConnection>(cx)
        .map_or(DEFAULT_MAX_RUNS_PER_CONNECTION, |max| max.0);
    upgrade
        .protocols([RUNTIME_PROTOCOL])
        .on_upgrade(move |socket| async move {
            let (sink, stream) = socket.split();
            let sink = sink.with(|text: String| {
                std::future::ready(Ok::<_, topcoat_core::error::Error>(Message::text(text)))
            });
            let stream = stream.filter_map(|message| {
                std::future::ready(match message {
                    Ok(Message::Text(text)) => Some(Ok(text.to_string())),
                    Ok(Message::Close(_)) | Err(_) => Some(Err(())),
                    Ok(_) => None,
                })
            });
            session::run(connection, sink, stream, max_runs).await;
        })
}

#[cfg(test)]
mod tests {
    use topcoat_router::HeaderValue;

    use super::*;

    fn headers_with_protocols(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static(value),
        );
        headers
    }

    #[test]
    fn the_runtime_protocol_is_recognized_among_others() {
        assert!(requests_runtime_protocol(&headers_with_protocols(
            "topcoat-runtime"
        )));
        assert!(requests_runtime_protocol(&headers_with_protocols(
            "chat.v1, topcoat-runtime"
        )));
        assert!(!requests_runtime_protocol(&headers_with_protocols(
            "topcoat-runtime-v2"
        )));
        assert!(!requests_runtime_protocol(&HeaderMap::new()));
    }
}
