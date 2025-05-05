use alloy::{
    primitives::{Address, U256},
    providers::RootProvider,
    pubsub::PubSubFrontend,
};
use overlord_shared::sol_bindings::AaveOracle;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
use tracing::info;

#[derive(Debug, Clone)]
pub struct PriceCache {
    prices: HashMap<String, HashMap<Address, U256>>,
    trace_order: VecDeque<String>,
    max_traces: usize,
    overriden_traces: HashSet<String>,
}

impl PriceCache {
    pub fn new(max_traces: usize) -> Self {
        Self {
            prices: HashMap::new(),
            trace_order: VecDeque::with_capacity(max_traces),
            max_traces,
            overriden_traces: HashSet::new(),
        }
    }

    /// This function overwrites the current price for a given asset and trace_id.
    /// Useful for mimicking price calculations based on price update TXs that haven't
    /// been published yet. Returns true if successful, or false otherwise.
    pub async fn override_price(
        &mut self,
        trace_id: String,
        new_prices_by_asset: Vec<(Address, String, U256)>,
    ) -> bool {
        // Since we receive a list of candidates, each of them will try to override
        // the price for the same asset. If an override is already present for a trace,
        // then just ignore the new override and return quickly.
        // This check is placed first, because it will be hit many more times than
        // the emptyness of new_prices_by_asset (which comes next)
        if self.overriden_traces.contains(&trace_id) {
            return true;
        }

        // This should only happen on initial vega-rs runs. No prices to update.
        if new_prices_by_asset.is_empty() {
            return true;
        }

        // Get or create the prices HashMap for this trace_id
        let prices = self
            .prices
            .entry(trace_id.clone())
            .or_default();

        for (reserve, symbol, new_price) in new_prices_by_asset.iter() {
            prices.insert(*reserve, *new_price);
            info!(
                "Successfully override {} price cache for {} (new value = {})",
                trace_id, symbol, new_price,
            );
        }

        // Mark the trace as overriden
        self.overriden_traces.insert(trace_id.clone());
        self.trace_order.push_back(trace_id.clone());

        // If we have reached the maximum number of traces, drop the oldest one
        if self.trace_order.len() >= self.max_traces {
            if let Some(oldest_trace) = self.trace_order.pop_front() {
                info!("Dropping prices cached for {}", oldest_trace);
                self.prices.remove(&oldest_trace);
                self.overriden_traces.remove(&oldest_trace);
            }
        }

        true
    }

    pub async fn get_price(
        &mut self,
        reserve: Address,
        trace_id: Option<String>,
        oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, Arc<RootProvider<PubSubFrontend>>>,
    ) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
        if trace_id.is_none() || trace_id == Some("initial-run".to_string()) {
            // This means the caller wants the actual price, not an overriden one
            // the caller is probably bpchecker or vega is initializing,
            // so we don't care about caching the price
            return match oracle.getAssetPrice(reserve).call().await {
                Ok(price_response) => Ok(price_response._0),
                Err(e) => Err(format!("Couldn't fetch price for {}: {}", reserve, e).into()),
            };
        }

        let trace_id = trace_id.expect("trace_id should be Some - checked above");

        // Check if there are overrides for this price
        if let Some(prices) = self.prices.get(&trace_id) {
            // If there are overrides, and the price is present, return it
            if let Some(&price) = prices.get(&reserve) {
                return Ok(price);
            }

            // If there are overrides, but the price is not present, fetch it
            let price = match oracle.getAssetPrice(reserve).call().await {
                Ok(price_response) => price_response._0,
                Err(e) => return Err(format!("Couldn't fetch price for {}: {}", reserve, e).into()),
            };

            // And update it for future calls within this trace_id context
            if let Some(prices) = self.prices.get_mut(&trace_id) {
                prices.insert(reserve, price);
            }

            return Ok(price);
        }
        Err(format!(
            "price for {} within trace_id {} not found in the price cache",
            reserve, trace_id
        )
        .into())
    }
}
