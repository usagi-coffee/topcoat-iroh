use std::{cell::RefCell, future::poll_fn, pin::Pin};

use futures_util::task::AtomicWaker;
use iroh::{
    EndpointAddr, EndpointId,
    endpoint::{Connection, RecvStream, SendStream},
};
use iroh_tickets::endpoint::EndpointTicket;
use wasm_bindgen::prelude::*;

use crate::{BrowserEndpoint, js_error};

const ALPN: &[u8] = b"topcoat/http/1";
const MAX_HEAD_SIZE: usize = 64 * 1024;

#[wasm_bindgen]
impl BrowserEndpoint {
    /// Connects to an app using an endpoint ID/address or a separate ticket.
    /// No HTTP bootstrap is needed.
    ///
    /// # Errors
    ///
    /// Returns an error unless exactly one endpoint or ticket is supplied, if
    /// that value is invalid, or if the connection fails.
    #[wasm_bindgen(js_name = connectHttp)]
    pub async fn connect_http(
        &self,
        endpoint: Option<String>,
        ticket: Option<String>,
    ) -> Result<HttpConnection, JsValue> {
        let endpoint = endpoint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let ticket = ticket
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let address = match (endpoint, ticket) {
            (Some(endpoint), None) => match endpoint.parse::<EndpointId>() {
                Ok(id) => EndpointAddr::from(id),
                Err(_) => serde_json::from_str(endpoint).map_err(js_error)?,
            },
            (None, Some(ticket)) => ticket
                .parse::<EndpointTicket>()
                .map_err(js_error)?
                .endpoint_addr()
                .clone(),
            _ => return Err(js_error("supply either an endpoint or a ticket")),
        };
        let connection = self
            .endpoint
            .connect(address, ALPN)
            .await
            .map_err(js_error)?;
        Ok(HttpConnection { connection })
    }
}

/// A connection that carries independent streamed app requests.
#[wasm_bindgen]
pub struct HttpConnection {
    connection: Connection,
}

#[wasm_bindgen]
impl HttpConnection {
    /// Whether the peer or transport has closed this connection.
    #[must_use]
    #[wasm_bindgen(js_name = isClosed)]
    pub fn is_closed(&self) -> bool {
        self.connection.close_reason().is_some()
    }

    /// Opens a request and sends its JSON metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized metadata or a failed stream.
    pub async fn open(&self, head: &str) -> Result<HttpRequest, JsValue> {
        if head.len() > MAX_HEAD_SIZE {
            return Err(js_error("request headers too large"));
        }
        let (mut send, receive) = self.connection.open_bi().await.map_err(js_error)?;
        let size = u32::try_from(head.len()).map_err(js_error)?;
        send.write_all(&size.to_be_bytes())
            .await
            .map_err(js_error)?;
        send.write_all(head.as_bytes()).await.map_err(js_error)?;
        Ok(HttpRequest {
            streams: RefCell::new(Some((send, receive))),
            reading: AtomicWaker::new(),
            writing: AtomicWaker::new(),
        })
    }

    /// Closes all requests on the connection.
    pub fn close(&self) {
        self.connection.close(0u32.into(), b"app connection ended");
    }
}

/// A streamed request body and response. Read and write one chunk at a time.
#[wasm_bindgen]
pub struct HttpRequest {
    streams: RefCell<Option<(SendStream, RecvStream)>>,
    reading: AtomicWaker,
    writing: AtomicWaker,
}

impl HttpRequest {
    async fn read_bytes(&self, bytes: &mut [u8]) -> Result<usize, JsValue> {
        poll_fn(|cx| {
            self.reading.register(cx.waker());
            let mut streams = self.streams.borrow_mut();
            let (_, receive) = streams
                .as_mut()
                .ok_or_else(|| js_error("request cancelled"))?;
            receive.poll_read(cx, bytes).map_err(js_error)
        })
        .await
    }

    async fn read_exact(&self, mut bytes: &mut [u8]) -> Result<(), JsValue> {
        while !bytes.is_empty() {
            let size = self.read_bytes(bytes).await?;
            if size == 0 {
                return Err(js_error("response ended before its headers"));
            }
            bytes = &mut bytes[size..];
        }
        Ok(())
    }
}

#[wasm_bindgen]
impl HttpRequest {
    /// Writes a request body chunk with transport backpressure.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream closes or the request is cancelled.
    pub async fn write(&self, bytes: &[u8]) -> Result<(), JsValue> {
        let mut bytes = bytes;
        while !bytes.is_empty() {
            let size = poll_fn(|cx| {
                self.writing.register(cx.waker());
                let mut streams = self.streams.borrow_mut();
                let (send, _) = streams
                    .as_mut()
                    .ok_or_else(|| js_error("request cancelled"))?;
                Pin::new(send).poll_write(cx, bytes).map_err(js_error)
            })
            .await?;
            if size == 0 {
                return Err(js_error("request stream closed"));
            }
            bytes = &bytes[size..];
        }
        Ok(())
    }

    /// Finishes the request body.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream is closed or cancelled.
    pub fn finish(&self) -> Result<(), JsValue> {
        let mut streams = self.streams.borrow_mut();
        let (send, _) = streams
            .as_mut()
            .ok_or_else(|| js_error("request cancelled"))?;
        send.finish().map_err(js_error)
    }

    /// Reads the response's JSON metadata. Call once, before reading its body.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized headers, invalid UTF-8, or a failed stream.
    pub async fn head(&self) -> Result<String, JsValue> {
        let mut size = [0; 4];
        self.read_exact(&mut size).await?;
        let size = u32::from_be_bytes(size) as usize;
        if size > MAX_HEAD_SIZE {
            return Err(js_error("response headers too large"));
        }
        let mut head = vec![0; size];
        self.read_exact(&mut head).await?;
        String::from_utf8(head).map_err(js_error)
    }

    /// Reads the next response body chunk, or returns undefined at EOF.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream fails or the request is cancelled.
    pub async fn read(&self) -> Result<Option<Vec<u8>>, JsValue> {
        let mut bytes = vec![0; 64 * 1024];
        let size = self.read_bytes(&mut bytes).await?;
        if size == 0 {
            return Ok(None);
        }
        bytes.truncate(size);
        Ok(Some(bytes))
    }

    /// Cancels both directions and wakes pending reads and writes.
    pub fn cancel(&self) {
        if let Some((mut send, mut receive)) = self.streams.borrow_mut().take() {
            let _ = send.reset(1u32.into());
            let _ = receive.stop(1u32.into());
        }
        self.reading.wake();
        self.writing.wake();
    }
}
