use awc::Client;
use awc::error::SendRequestError;
use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;
use tokio::sync::OnceCell;

static GLOBAL_TLS_CONFIG: OnceCell<Arc<ClientConfig>> = OnceCell::const_new();
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn httping_target(target: &str) -> Result<std::time::Duration, SendRequestError> {
    let tls_config = GLOBAL_TLS_CONFIG
        .get_or_init(|| async {
            let mut root_store = RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.to_owned());

            let mut config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();

            let protos = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            config.alpn_protocols = protos;

            Arc::new(config)
        })
        .await;

    let local = tokio::task::LocalSet::new();

    local
        .run_until(async move {
            let client = Client::builder()
                .connector(awc::Connector::new().rustls_0_23(tls_config.clone()))
                .timeout(PING_TIMEOUT)
                .no_default_headers()
                .finish();

            let request = client.get(target).append_header(("User-Agent", "awc/3.0"));

            let start = std::time::Instant::now();

            match request.send().await {
                Ok(_) => Ok(start.elapsed()),
                Err(e) => Err(e),
            }
        })
        .await
}
