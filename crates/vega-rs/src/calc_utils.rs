use alloy::{
    primitives::{address, Address, U256},
    providers::RootProvider,
    pubsub::PubSubFrontend,
    sol,
};
use futures::future::join_all;
use std::{collections::HashMap, sync::Arc};
use tokio::task;
use tracing::warn;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    AaveV3Pool,
    "src/abis/aave_v3_pool.json"
);

const HF_MIN_THRESHOLD: u128 = 1_000_000_000_000_000_000u128;

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
        let task = task::spawn(async move {
            let mut bucket_results = HashMap::new();
            for address in bucket {
                let result = pool.getUserAccountData(address).call().await;
                match result {
                    Ok(data) => {
                        if data.healthFactor < U256::from(HF_MIN_THRESHOLD) {
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
