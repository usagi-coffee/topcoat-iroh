use std::{fs, path::Path};

use iroh_base::{EndpointAddr, EndpointId, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize};

use super::Error;

/// Native build settings. Only the public destination belongs in the launcher.
#[derive(Serialize)]
pub(super) struct ServerIdentity {
    version: u32,
    secret: Option<SecretKey>,
    endpoint: EndpointId,
    peers: Vec<EndpointId>,
}

impl ServerIdentity {
    pub(super) fn load(
        path: &Path,
        secret: Option<&str>,
        endpoint: Option<&str>,
        ticket: Option<&str>,
        peers: &[String],
    ) -> Result<Self, Error> {
        let destination = match (endpoint, ticket) {
            (Some(endpoint), None) => Some(match endpoint.trim().parse::<EndpointId>() {
                Ok(id) => id,
                Err(_) => {
                    serde_json::from_str::<EndpointAddr>(endpoint)
                        .map_err(|_| "invalid --iroh-endpoint")?
                        .id
                }
            }),
            (None, Some(ticket)) => Some(
                ticket
                    .trim()
                    .parse::<EndpointTicket>()
                    .map_err(|_| "invalid --iroh-ticket")?
                    .endpoint_addr()
                    .id,
            ),
            (None, None) => None,
            (Some(_), Some(_)) => {
                return Err("supply either --iroh-endpoint or --iroh-ticket".into());
            }
        };
        // A public destination without a key deliberately avoids the private cache.
        // The deployment supplies TOPCOAT_IROH_SECRET at runtime instead.
        let cached = if secret.is_none() && destination.is_some() {
            None
        } else {
            match fs::read(path) {
                Ok(bytes) => {
                    let cached: CachedIdentity = serde_json::from_slice(&bytes)?;
                    if cached.version != 1 {
                        return Err("unsupported saved iroh identity version".into());
                    }
                    Some(cached)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            }
        };
        let secret = match secret {
            Some(secret) => Some(
                secret
                    .trim()
                    .parse::<SecretKey>()
                    .map_err(|_| "invalid --iroh-secret")?,
            ),
            None if destination.is_some() => None,
            None => Some(
                cached
                    .as_ref()
                    .map_or_else(SecretKey::generate, |cached| cached.secret.clone()),
            ),
        };
        let endpoint = match (&secret, destination) {
            (Some(secret), Some(endpoint)) if secret.public() != endpoint => {
                return Err(
                    "--iroh-secret does not match the endpoint in --iroh-endpoint/--iroh-ticket"
                        .into(),
                );
            }
            (Some(secret), _) => secret.public(),
            (None, Some(endpoint)) => endpoint,
            (None, None) => unreachable!(),
        };
        let peers = if peers.is_empty() {
            cached.map(|cached| cached.peers).unwrap_or_default()
        } else {
            peers
                .iter()
                .map(|peer| peer.parse().map_err(|_| "invalid --iroh-peer"))
                .collect::<Result<_, _>>()?
        };
        Ok(Self {
            version: 1,
            secret,
            endpoint,
            peers,
        })
    }

    pub(super) fn endpoint(&self) -> EndpointId {
        self.endpoint
    }

    pub(super) fn save(&self, path: &Path) -> Result<(), Error> {
        let Some(secret) = &self.secret else {
            return Ok(());
        };
        let parent = path.parent().ok_or("identity file has no parent")?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(
            &mut file,
            &CachedIdentity {
                version: self.version,
                secret: secret.clone(),
                peers: self.peers.clone(),
            },
        )?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct CachedIdentity {
    version: u32,
    secret: SecretKey,
    peers: Vec<EndpointId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_identity_and_peers_until_explicitly_replaced() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("identity.json");
        let peer = SecretKey::generate().public();
        let identity = ServerIdentity::load(&path, None, None, None, &[peer.to_string()]).unwrap();
        identity.save(&path).unwrap();
        let reused = ServerIdentity::load(&path, None, None, None, &[]).unwrap();
        assert_eq!(identity.endpoint(), reused.endpoint());
        assert_eq!(reused.peers, [peer]);
        let replaced =
            ServerIdentity::load(&path, Some(&"23".repeat(32)), None, None, &[]).unwrap();
        assert_ne!(replaced.endpoint(), identity.endpoint());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        }
        fs::write(&path, "invalid").unwrap();
        assert!(ServerIdentity::load(&path, None, None, None, &[]).is_err());
    }

    #[test]
    fn public_destinations_require_matching_keys_or_runtime_secrets() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("identity.json");
        let secret_text = "23".repeat(32);
        let secret: SecretKey = secret_text.parse().unwrap();
        let endpoint = secret.public().to_string();
        let ticket = EndpointTicket::new(secret.public().into()).to_string();
        let address = serde_json::to_string(&EndpointAddr::from(secret.public())).unwrap();
        for (endpoint, ticket) in [
            (Some(endpoint.as_str()), None),
            (Some(address.as_str()), None),
            (None, Some(ticket.as_str())),
        ] {
            let external = ServerIdentity::load(&path, None, endpoint, ticket, &[]).unwrap();
            assert_eq!(external.endpoint(), secret.public());
            assert!(external.secret.is_none());
            external.save(&path).unwrap();
            assert!(!path.exists());
            assert!(
                ServerIdentity::load(&path, Some(&"42".repeat(32)), endpoint, ticket, &[]).is_err()
            );
            let embedded =
                ServerIdentity::load(&path, Some(&secret_text), endpoint, ticket, &[]).unwrap();
            assert_eq!(embedded.secret.unwrap().public(), secret.public());
        }
        // Even a preexisting generated key must not leak into an external-secret build.
        let cached = ServerIdentity::load(&path, None, None, None, &[]).unwrap();
        cached.save(&path).unwrap();
        let before = fs::read(&path).unwrap();
        let external = ServerIdentity::load(&path, None, Some(&endpoint), None, &[]).unwrap();
        assert!(external.secret.is_none());
        external.save(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
