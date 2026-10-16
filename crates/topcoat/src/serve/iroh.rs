use std::{env, future::Future, io};

use iroh::{Endpoint, SecretKey, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;

use crate::{
    router::{IROH_HTTP_ALPN, IrohService, RouterService},
    runtime::{IROH_RUNTIME_ALPN, IrohTransport},
};

#[derive(serde::Deserialize)]
struct Configuration {
    version: u32,
    secret: Option<SecretKey>,
    endpoint: iroh::EndpointId,
    peers: Vec<iroh::EndpointId>,
}

impl Configuration {
    fn embedded() -> io::Result<Self> {
        let json = option_env!("TOPCOAT_BUNDLE_IROH")
            .ok_or_else(|| io::Error::other("missing bundled iroh settings"))?;
        let config: Self = serde_json::from_str(json).map_err(io::Error::other)?;
        if config.version != 1 {
            return Err(io::Error::other("unsupported bundled iroh settings"));
        }
        Ok(config)
    }

    fn server_secret(&self, environment: Option<&str>) -> io::Result<SecretKey> {
        let secret = match environment {
            Some(value) => value
                .trim()
                .parse()
                .map_err(|_| io::Error::other("invalid TOPCOAT_IROH_SECRET"))?,
            None => self.secret.clone().ok_or_else(|| {
                io::Error::other(
                    "set TOPCOAT_IROH_SECRET to the key matching the bundled server endpoint",
                )
            })?,
        };
        if secret.public() != self.endpoint {
            return Err(io::Error::other(
                "server secret does not match the bundled iroh endpoint",
            ));
        }
        Ok(secret)
    }
}

pub(super) fn configured() -> bool {
    option_env!("TOPCOAT_BUNDLE_IROH").is_some() && env::var_os("TOPCOAT_DEV_URL").is_none()
}

pub(super) async fn start(
    service: RouterService,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    let config = Configuration::embedded()?;
    let environment = match env::var("TOPCOAT_IROH_SECRET") {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(_) => return Err(io::Error::other("invalid TOPCOAT_IROH_SECRET")),
    };
    let secret = config.server_secret(environment.as_deref())?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret)
        .bind()
        .await
        .map_err(io::Error::other)?;
    let transport = IrohTransport::new(
        endpoint.clone(),
        "/__topcoat/topcoat_runtime_iroh_browser.js",
    )
    .allow_peers(config.peers.iter().copied());
    let (router, timeout) = service.into_iroh_parts();
    let listener = iroh::protocol::Router::builder(endpoint.clone())
        .accept(
            IROH_HTTP_ALPN,
            IrohService::new(router)
                .allow_peers(config.peers)
                .request_context(transport.clone()),
        )
        .accept(IROH_RUNTIME_ALPN, transport)
        .spawn();
    let mut shutdown = std::pin::pin!(shutdown);
    tokio::select! {
        () = endpoint.online() => {
            println!("App address: {}", endpoint.id());
            println!("App ticket: {}", EndpointTicket::from(endpoint.addr()));
            shutdown.as_mut().await;
        }
        () = &mut shutdown => {}
    }
    tokio::time::timeout(timeout, listener.shutdown())
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_embedded_and_environment_keys_before_starting() {
        let secret_text = "23".repeat(32);
        let secret: SecretKey = secret_text.parse().unwrap();
        let mut config = Configuration {
            version: 1,
            endpoint: secret.public(),
            secret: None,
            peers: Vec::new(),
        };
        assert!(config.server_secret(None).is_err());
        assert!(config.server_secret(Some("")).is_err());
        assert!(config.server_secret(Some("invalid")).is_err());
        assert!(config.server_secret(Some(&"42".repeat(32))).is_err());
        assert_eq!(
            config.server_secret(Some(&secret_text)).unwrap().public(),
            config.endpoint
        );
        config.secret = Some(secret);
        assert_eq!(
            config.server_secret(None).unwrap().public(),
            config.endpoint
        );
        // An invalid environment override must not silently fall back to the embedded key.
        assert!(config.server_secret(Some(&"42".repeat(32))).is_err());
    }
}
