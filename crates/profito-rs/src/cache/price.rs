use super::super::sol_bindings::AaveOracle;
use alloy::{
    primitives::{Address, U256},
    providers::RootProvider,
    pubsub::PubSubFrontend,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
use tracing::{info, warn, error};

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
        // This should only happen on initial vega-rs runs. No prices to update.
        if new_prices_by_asset.is_empty() {
            return true;
        }

        // Now that override_price is called _before_ processing candidates, it should never
        // happen that we already have a trace present in the cache.
        // I don't want to kill the program in this case, so I'm returning true as if nothing
        // happened. But I'm logging an error nonetheless because this would be a most curious
        // occurrence.
        if self.overriden_traces.contains(&trace_id) {
            error!("trace_id {} was present in the price cache", trace_id);
            return true;
        }

        // Get or create the prices HashMap for this trace_id
        let prices = self.prices.entry(trace_id.clone()).or_insert_with(|| HashMap::new());

        for (reserve, symbol, new_price) in new_prices_by_asset.iter() {
            prices.insert(*reserve, *new_price);
            info!(
                "Successfully override {} price cache for {} (new value = {})",
                trace_id, symbol, new_price,
            );
        }

        // Mark the trace as overriden
        // This hashset get's it's element removed when the trace is dropped from the cache
        // at price_cache.get_price()
        self.overriden_traces.insert(trace_id.clone());

        true
    }

    pub async fn get_price(
        &mut self,
        reserve: Address,
        trace_id: String,
        oracle: AaveOracle::AaveOracleInstance<PubSubFrontend, Arc<RootProvider<PubSubFrontend>>>,
    ) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
        // Check if price exists for this trace_id
        if let Some(prices) = self.prices.get(&trace_id) {
            if let Some(&price) = prices.get(&reserve) {
                return Ok(price);
            }
        }

        // Fetch new price
        let price = match oracle.getAssetPrice(reserve).call().await {
            Ok(price_response) => price_response._0,
            Err(e) => return Err(format!("Couldn't fetch price for {}: {}", reserve, e).into()),
        };

        // Add new trace_id if not exists
        if !self.prices.contains_key(&trace_id) {
            if self.trace_order.len() >= self.max_traces {
                if let Some(oldest_trace) = self.trace_order.pop_front() {
                    info!("Dropping prices cached for {}", oldest_trace);
                    self.prices.remove(&oldest_trace);
                    self.overriden_traces.remove(&oldest_trace);
                }
            }
            self.trace_order.push_back(trace_id.clone());
            self.prices.insert(trace_id.clone(), HashMap::new());
        }

        // Update price
        if let Some(prices) = self.prices.get_mut(&trace_id) {
            prices.insert(reserve, price);
        }

        Ok(price)
    }
}
