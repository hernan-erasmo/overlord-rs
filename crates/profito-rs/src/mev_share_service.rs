use ethers_core::{
    k256::ecdsa::SigningKey, rand::thread_rng, types::{transaction::eip2718::TypedTransaction, H256}
};
use ethers_signers::{LocalWallet, Signer, Wallet};
use jsonrpsee::http_client::{transport::{Error as HttpError, HttpBackend}, HttpClient, HttpClientBuilder};
use mev_share::rpc::{BundleItem, FlashbotsSigner, FlashbotsSignerLayer, MevApiClient, SendBundleRequest};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::{util::MapErr, ServiceBuilder};
use tracing::info;

type MevShareClient = HttpClient<MapErr<FlashbotsSigner<Wallet<SigningKey>, HttpBackend>, fn(Box<dyn std::error::Error + Send + Sync>) -> HttpError>>;

static MEVSHARE_CLIENT: OnceCell<Arc<MevShareClient>> = OnceCell::new();

#[derive(Clone)]
pub struct MevShareService {
    initialization: Arc<Mutex<()>>,
    fb_signer: LocalWallet,
    tx_signer: LocalWallet,
}

impl Default for MevShareService {
    fn default() -> Self {
        Self::new()
    }
}

impl MevShareService {
    pub fn new() -> Self {
        Self {
            fb_signer: LocalWallet::new(&mut thread_rng()),
            tx_signer: LocalWallet::new(&mut thread_rng()),
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

        // Set up flashbots-style auth middleware
        let signing_middleware = FlashbotsSignerLayer::new(self.fb_signer.clone());
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

    pub async fn submit_simple_liquidation_bundle(
        &self,
        pub_tx: H256,
        foxdie_tx: TypedTransaction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let signature = self.tx_signer.sign_transaction(&foxdie_tx.clone().into()).await?;
        let bytes = foxdie_tx.rlp_signed(&signature);
        let bundle_body = vec![
            BundleItem::Hash { hash: pub_tx },
            BundleItem::Tx { tx: bytes, can_revert: false },
        ];
        let bundle = SendBundleRequest {
            bundle_body, ..Default::default()
        };

        /*
            Uncomment when ready

            // Send bundle
            let client = &*self.get_client().await?;
            let send_res = MevApiClient::send_bundle(client, bundle.clone()).await;
            info!("Got a bundle response: {:?}", send_res);

            // Simulate bundle
            let sim_res = MevApiClient::sim_bundle(client, bundle.clone(), Default::default()).await;
            info!("Got a simulation response: {:?}", sim_res);
         */

        Ok(())
    }
}
