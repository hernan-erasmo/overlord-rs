use alloy::{
    primitives::{address, Address, U256},
    providers::RootProvider,
    pubsub::PubSubFrontend,
    sol,
};
use futures::future::join_all;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::mpsc,
    task::{self, JoinHandle},
};
use tracing::warn;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(serde::Serialize)]
    #[sol(rpc)]
    AaveV3Pool,
    "src/abis/aave_v3_pool.json"
);

const HF_MIN_THRESHOLD: u128 = 1_000_000_000_000_000_000u128;

// This number should be at least as big as the number of address buckets
// passed to get_hf_for_users()
const USER_ACCOUNT_DATA_WRITER_CHANNEL_SIZE: usize = 1000;
const PROFITO_INBOUND_ENDPOINT: &str = "ipc:///tmp/profito_inbound";

pub struct UserAccountDataWriter {
    pub tx: mpsc::Sender<AaveV3Pool::getUserAccountDataReturn>,
}

impl UserAccountDataWriter {
    pub fn new() -> (Self, JoinHandle<()>) {
        let (tx, mut rx) = mpsc::channel(USER_ACCOUNT_DATA_WRITER_CHANNEL_SIZE);
        let handle = tokio::spawn(async move {
            let context = zmq::Context::new();
            let socket = context.socket(zmq::PUSH).unwrap();
            if let Err(e) = socket.connect(PROFITO_INBOUND_ENDPOINT) {
                warn!("Failed to connect to profito-rs socket: {:?}", e);
                return;
            }
            while let Some(data) = rx.recv().await {
                if let Ok(bytes) = bincode::serialize(&data) {
                    if let Err(e) = socket.send(&bytes, 0) {
                        warn!("Failed to send user_account_data to profito-rs: {:?}", e);
                    }
                }
            }
        });
        (Self { tx }, handle)
    }

    pub async fn send(&self, data: AaveV3Pool::getUserAccountDataReturn) {
        if let Err(e) = self.tx.send(data).await {
            warn!("Failed to send alert to UserAccountData writer: {:?}", e);
        }
    }
}

pub struct HealthFactorCalculationResults {
    pub raw_results: HashMap<Address, U256>,
    pub under_1_hf: HashMap<Address, U256>,
}

/// Given a array of user address buckets and a provider, query the AAVE v3's Pool contract
/// and return a structure with the HF of all addresses, as well as a separate attribute with
/// only underwater users
pub async fn get_hf_for_users<F>(
    address_buckets: Vec<Vec<Address>>,
    provider: &RootProvider<PubSubFrontend>,
    alert_callback: Option<F>,
) -> HealthFactorCalculationResults
where
    F: Fn(Address, U256, U256) + Send + Sync + 'static,
{
    let (user_data_writer, user_data_writer_handle) = UserAccountDataWriter::new();
    let user_data_writer = Arc::new(user_data_writer);
    let mut tasks = vec![];
    let pool = AaveV3Pool::new(
        address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
        provider.clone(),
    );
    let pool = Arc::new(pool);
    let alert_callback = Arc::new(alert_callback);
    for bucket in address_buckets {
        let pool = pool.clone();
        let alert_callback = alert_callback.clone();
        let user_data_writer = user_data_writer.clone();
        let task = task::spawn(async move {
            let mut bucket_results = HashMap::new();
            for address in bucket {
                let result = pool.getUserAccountData(address).call().await;
                match result {
                    Ok(data) => {
                        if data.healthFactor < U256::from(HF_MIN_THRESHOLD) {
                            user_data_writer.send(data.clone()).await;
                            if let Some(cb) = &*alert_callback {
                                cb(address, data.healthFactor, data.totalCollateralBase);
                            }
                        }
                        bucket_results.insert(address, data.healthFactor);
                    }
                    Err(e) => warn!("Couldn't calculate address HF: {:?}", e),
                }
            }
            bucket_results
        });
        tasks.push(task);
    }
    let bucket_aggregate_results: Vec<HashMap<Address, U256>> = join_all(tasks)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();
    let _ = user_data_writer_handle.await;
    let mut raw_results = HashMap::new();
    let mut under_1_hf = HashMap::new();
    for bucket_results in bucket_aggregate_results {
        raw_results.extend(bucket_results);
    }
    for (address, hf) in raw_results.iter() {
        if *hf < U256::from(HF_MIN_THRESHOLD) {
            under_1_hf.insert(*address, *hf);
        }
    }
    HealthFactorCalculationResults {
        raw_results,
        under_1_hf,
    }
}
