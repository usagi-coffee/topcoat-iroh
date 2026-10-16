use std::time::Duration;

use topcoat_core::context::Cx;
use topcoat_router::{HeaderName, HeaderValue, request::headers, response::response_headers};

use crate::{Token, TokenStore, TokenStoreFuture};

/// Carries session tokens in headers for the static iroh browser launcher.
///
/// Reads `Authorization: Bearer <token>` and sends token changes through
/// `X-Topcoat-Session`. The launcher persists tokens in `IndexedDB`, attaches
/// them to same-origin app requests, and removes expired tokens from use.
/// Unlike an HTTP-only cookie, this storage is accessible to scripts on
/// the launcher's origin. Host each app on its own trusted origin.
///
/// Configure with `SessionConfig::builder().token_store(HeaderTokenStore).build()`.
/// Session records and their expiration still belong in application storage.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderTokenStore;

impl TokenStore for HeaderTokenStore {
    fn read<'a>(&'a self, cx: &'a Cx) -> TokenStoreFuture<'a, Option<Token>> {
        Box::pin(async move {
            Ok(headers(cx)
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .and_then(|value| Token::decode(value).ok()))
        })
    }

    fn write<'a>(
        &'a self,
        cx: &'a Cx,
        token: Token,
        max_age: Duration,
    ) -> TokenStoreFuture<'a, ()> {
        Box::pin(async move {
            // Token::encode only produces URL-safe base64 characters.
            let value = format!(
                "{{\"token\":\"{}\",\"maxAge\":{}}}",
                token.encode(),
                max_age.as_secs()
            );
            response_headers(cx).append(
                HeaderName::from_static("x-topcoat-session"),
                HeaderValue::from_str(&value)?,
            );
            Ok(())
        })
    }

    fn delete<'a>(&'a self, cx: &'a Cx) -> TokenStoreFuture<'a, ()> {
        Box::pin(async move {
            response_headers(cx).append(
                HeaderName::from_static("x-topcoat-session"),
                HeaderValue::from_static("{\"token\":\"\",\"maxAge\":0}"),
            );
            Ok(())
        })
    }
}
