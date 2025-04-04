use ethers_core::{
    rand::thread_rng,
    k256::ecdsa::SigningKey
};
use ethers_signers::{LocalWallet, Wallet};
use jsonrpsee::http_client::{transport::{Error as HttpError, HttpBackend}, HttpClient, HttpClientBuilder};
use mev_share::rpc::{FlashbotsSigner, FlashbotsSignerLayer};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::{util::MapErr, ServiceBuilder};

type MevShareClient = HttpClient<MapErr<FlashbotsSigner<Wallet<SigningKey>, HttpBackend>, fn(Box<dyn std::error::Error + Send + Sync>) -> HttpError>>;

static MEVSHARE_CLIENT: OnceCell<Arc<MevShareClient>> = OnceCell::new();

#[derive(Clone)]
pub struct MevShareService {
    initialization: Arc<Mutex<()>>,
}

impl Default for MevShareService {
    fn default() -> Self {
        Self::new()
    }
}

impl MevShareService {
    pub fn new() -> Self {
        Self {
            initialization: Arc::new(Mutex::new(())),
        }
    }

    pub async fn get_client(
        &self,
    ) -> Result<Arc<MevShareClient>, Box<dyn std::error::Error>> {
        if let Some(client) = MEVSHARE_CLIENT.get() {
            return Ok((*client).clone());
        }
        let _lock = self.initialization.lock().await;

        if let Some(client) = MEVSHARE_CLIENT.get() {
            return Ok(client.clone());
        }

        // The signer used to authenticate bundles
        let fb_signer = LocalWallet::new(&mut thread_rng());

        // The signer used to sign our transactions
        let tx_signer = LocalWallet::new(&mut thread_rng());

        // Set up flashbots-style auth middleware
        let signing_middleware = FlashbotsSignerLayer::new(fb_signer);
        let service_builder = ServiceBuilder::new()
            // map signer errors to http errors
            .map_err(HttpError::Http as fn(Box<dyn std::error::Error + Send + Sync>) -> HttpError)
            .layer(signing_middleware);

        // Set up the rpc client
        let url = "https://relay.flashbots.net:443";
        let client = HttpClientBuilder::default()
            .set_middleware(service_builder)
            .build(url)
            .expect("Failed to create http client");

        MEVSHARE_CLIENT
            .set(client.clone().into())
            .map_err(|_| "Failed to SET Mev-Share client on cache")?;
        Ok(client.into())
    }
}
