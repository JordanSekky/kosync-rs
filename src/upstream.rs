use std::time::Duration;

use reqwest::{Client, header::ACCEPT};
use rustls::{ClientConfig, RootCertStore};
use url::Url;

use crate::{
    auth::{AUTH_KEY, AUTH_USER, AuthHeaders},
    error::{Error, Result},
    handlers::kosync::UpdateProgressRequest,
};

/// What the reference server's API versioning keys on. The real client sends
/// it on every request.
const KOREADER_ACCEPT: &str = "application/vnd.koreader.v1+json";

/// The KOReader client gives up on a progress push after this long.
const PUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Another kosync server that pushes get forwarded to.
#[derive(Debug, Clone)]
pub struct Upstream {
    /// The server's base URL; a path prefix on it is kept.
    pub url: Url,
    pub client: Client,
}

impl Upstream {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            client: build_client(),
        }
    }

    /// Replays a push against this server, with the credentials the client
    /// sent.
    ///
    /// Fails with [`Error::Upstream`] if the server can't be reached or
    /// answers with anything other than a success status.
    #[tracing::instrument(skip_all, fields(host = self.url.host_str()))]
    pub async fn forward_push(
        &self,
        auth: &AuthHeaders,
        request: &UpdateProgressRequest,
    ) -> Result<()> {
        let mut url = self.url.clone();
        url.path_segments_mut()
            .map_err(|()| Error::Server)?
            .pop_if_empty()
            .extend(["syncs", "progress"]);

        self.client
            .put(url)
            .header(ACCEPT, KOREADER_ACCEPT)
            .header(AUTH_USER, &auth.username)
            .header(AUTH_KEY, &auth.key)
            .timeout(PUSH_TIMEOUT)
            .json(request)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}

/// TLS is rustls verifying against Mozilla's root bundle compiled in through
/// `webpki-roots`, so it doesn't depend on the host's certificate store.
fn build_client() -> Client {
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    Client::builder()
        .tls_backend_preconfigured(tls)
        .build()
        .expect("Failed to build upstream HTTP client")
}
