use alloy::{
    primitives::{address, Address, U256},
    providers::RootProvider,
    pubsub::PubSubFrontend,
};
use futures::future::join_all;
use overlord_shared_types::{AaveV3Pool, UnderwaterUserEvent};
use std::{collections::HashMap, sync::Arc};
use tokio::{sync::broadcast, task};
use tracing::warn;

const HF_MIN_THRESHOLD: u128 = 1_000_000_000_000_000_000u128;

// From observations in profito, these are comparisons between collateral
// and expected profit:
// > 1e9 ~ $0.xx to $9.xx
// > 1e10 ~ $10.xx to $99.xx
// > 1e11 ~ $100.xx to $999.xx
// > 1e12 ~ $1000.xx and above
const MIN_REPORTABLE_COLLATERAL: f64 = 1e9;

pub struct UnderwaterUserEventBus {
    sender: broadcast::Sender<UnderwaterUserEvent>,
}

impl UnderwaterUserEventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<UnderwaterUserEvent> {
        self.sender.subscribe()
    }

    pub fn send(&self, event: UnderwaterUserEvent) {
        let _ = self.sender.send(event);
    }
}

pub struct HealthFactorCalculationResults {
    pub raw_results: HashMap<Address, U256>,
    pub under_1_hf: HashMap<Address, U256>,
}

/// Given a array of user address buckets and a provider, query the AAVE v3's Pool contract
/// and return a structure with the HF of all addresses, as well as a separate attribute with
/// only underwater users
pub async fn get_hf_for_users(
    address_buckets: Vec<Vec<Address>>,
    provider: &RootProvider<PubSubFrontend>,
    trace_id: Option<String>,
    tx_hash: Option<String>,
    inclusion_block: Option<String>,
    new_prices_by_asset: Vec<(Address, String, U256)>,
    event_bus: Option<Arc<UnderwaterUserEventBus>>,
) -> HealthFactorCalculationResults {
    let mut tasks = vec![];
    let pool = Arc::new(AaveV3Pool::new(
        address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
        provider.clone(),
    ));
    let trace_id = trace_id.clone();
    let tx_hash = tx_hash.clone();
    let inclusion_block = inclusion_block.clone();
    for bucket in address_buckets {
        let pool = pool.clone();
        let event_bus = event_bus.clone();
        let new_prices_by_asset = new_prices_by_asset.clone();
        let trace_id = trace_id
            .as_ref()
            .map(String::from)
            .unwrap_or_else(|| String::from("initial-run"));
        let tx_hash = tx_hash
            .as_ref()
            .map(String::from)
            .unwrap_or_else(|| String::from("initial-run"));
        let inclusion_block = inclusion_block
            .as_ref()
            .map(String::from)
            .unwrap_or_else(|| String::from("initial-run"));
        let task = task::spawn(async move {
            let mut bucket_results = HashMap::new();
            for address in bucket {
                let result = pool.getUserAccountData(address).call().await;
                match result {
                    Ok(data) => {
                        if data.healthFactor < U256::from(HF_MIN_THRESHOLD) && data.totalCollateralBase > U256::from(MIN_REPORTABLE_COLLATERAL) {
                            if let Some(bus) = &event_bus {
                                bus.send(UnderwaterUserEvent {
                                    address,
                                    trace_id: trace_id.clone(),
                                    tx_hash: tx_hash.clone(),
                                    inclusion_block: inclusion_block.clone(),
                                    total_collateral_base: data.totalCollateralBase,
                                    user_account_data: data.clone(),
                                    new_asset_prices: new_prices_by_asset.clone(),
                                });
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
