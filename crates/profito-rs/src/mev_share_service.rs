use alloy::primitives::Bytes;
use ethers_core::{
    k256::ecdsa::SigningKey,
    rand::thread_rng,
    types::{transaction::eip2718::TypedTransaction, Chain, H256, U64},
};
use ethers_signers::{LocalWallet, Signer, Wallet};
use jsonrpsee::http_client::{
    transport::{Error as HttpError, HttpBackend},
    HttpClient, HttpClientBuilder,
};
use mev_share::rpc::{
    BundleItem, FlashbotsSigner, FlashbotsSignerLayer, Inclusion, MevApiClient, SendBundleRequest,
    SendBundleResponse,
};
use once_cell::sync::OnceCell;
use std::{env, str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tower::{util::MapErr, ServiceBuilder};
use tracing::info;

type MevShareClient = HttpClient<
    MapErr<
        FlashbotsSigner<Wallet<SigningKey>, HttpBackend>,
        fn(Box<dyn std::error::Error + Send + Sync>) -> HttpError,
    >,
>;

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
            tx_signer: LocalWallet::from_str(&env::var("FOXDIE_OWNER_PK").unwrap())
                .unwrap()
                .with_chain_id(Chain::Mainnet),
            initialization: Arc::new(Mutex::new(())),
        }
    }

    pub async fn get_client(&self) -> Result<Arc<MevShareClient>, Box<dyn std::error::Error>> {
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
        pub_tx: Option<String>,
        raw_tx: Option<Bytes>,
        foxdie_tx: TypedTransaction,
        inclusion_block: String,
    ) -> Result<SendBundleResponse, Box<dyn std::error::Error>> {
        let signature = self
            .tx_signer
            .sign_transaction(&foxdie_tx.clone())
            .await?;
        let bytes = foxdie_tx.rlp_signed(&signature);
        let backrun_tx: BundleItem;
        if let Some(raw) = raw_tx {
            // Convert from alloy::primitives::Bytes to ethers_core::types::Bytes
            let ethers_bytes = ethers_core::types::Bytes::from(raw.to_vec());
            backrun_tx = BundleItem::Tx {
                tx: ethers_bytes,
                can_revert: false,
            };
        } else if let Some(pub_hash) = pub_tx {
            backrun_tx = BundleItem::Hash {
                hash: H256::from_str(&pub_hash)?,
            };
        } else {
            return Err("Didn't get a tx hash or raw data to backrun".to_string().into());
        };
        let bundle_body = vec![
            backrun_tx,
            BundleItem::Tx {
                tx: bytes,
                can_revert: false,
            },
        ];
        let block = U64::from(inclusion_block.parse::<u64>()?);
        let max_block = block + U64::from(5u64);
        let bundle = SendBundleRequest {
            bundle_body,
            inclusion: Inclusion {
                block,
                max_block: Some(max_block),
            },
            ..Default::default()
        };

        let client = &*self.get_client().await?;
        info!("Sending bundle: {:?}", bundle);
        match MevApiClient::send_bundle(client, bundle.clone()).await {
            Ok(res) => Ok(res),
            Err(e) => Err(format!("Error on send_bundle: {}", e).into()),
        }
    }
}
