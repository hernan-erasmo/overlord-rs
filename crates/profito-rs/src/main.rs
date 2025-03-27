mod cache;
mod calculations;
mod constants;
pub mod sol_bindings;
mod utils;

use alloy::{providers::RootProvider, pubsub::PubSubFrontend};
use cache::{PriceCache, ProviderCache};
use calculations::{get_best_liquidation_opportunity, get_reserves_list, get_reserves_data, calculate_user_account_data};
use constants::*;
use overlord_shared_types::UnderwaterUserEvent;
use sol_bindings::AaveOracle;
use std::{sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};
use utils::get_user_reserves_data;

fn _setup_logging() {
    let log_file = rolling::RollingFileAppender::new(
        Rotation::DAILY,
        "/var/log/overlord-rs",
        "profito-rs.log",
    );
    let file_writer = BoxMakeWriter::new(log_file);
    tracing_subscriber::fmt()
        .with_writer(file_writer)
        .with_timer(LocalTime::rfc_3339())
        .with_target(true)
        .init();
}

async fn process_uw_event(
    uw_event: UnderwaterUserEvent,
    provider_cache: Arc<ProviderCache>,
    price_cache: Arc<tokio::sync::Mutex<PriceCache>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let process_uw_event_timer = Instant::now();
    let provider = match provider_cache.get_provider().await {
        Ok(provider) => provider,
        Err(e) => {
            warn!("Failed to get the provider for uw processing: {e}");
            return Err(e);
        }
    };
    let user_reserve_data = get_user_reserves_data(provider.clone(), uw_event.address).await;
    if user_reserve_data.len() == 0 {
        return Err("User reserves data came back empty".into());
    };

    let aave_oracle: AaveOracle::AaveOracleInstance<
        PubSubFrontend,
        Arc<RootProvider<PubSubFrontend>>,
    > = AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone());

    let reserves_list = match get_reserves_list(provider.clone()).await {
        Ok(reserves_list) => reserves_list,
        Err(e) => {
            return Err(e)
        }
    };

    let reserves_data = match get_reserves_data(provider.clone()).await {
        Ok(reserves_data) => reserves_data,
        Err(e) => {
            return Err(e)
        }
    };

    let (
        total_collateral_in_base_currency,
        total_debt_in_base_currency,
        health_factor_v33
    ) = match calculate_user_account_data(
            price_cache.clone(),
            provider.clone(),
            uw_event.address,
            reserves_list.clone(),
            reserves_data.clone(),
            Some(uw_event.trace_id.clone()),
        ).await {
            Ok((collateral, debt, hf)) => (collateral, debt, hf),
            Err(e) => {
                return Err(format!("Error calculating user account data: {}", e).into());
            }
        };

    if let Some(best_pair) = get_best_liquidation_opportunity(
        user_reserve_data,
        reserves_data,
        uw_event.address,
        health_factor_v33,
        total_debt_in_base_currency,
        price_cache,
        provider.clone(),
        uw_event.trace_id.clone(),
        aave_oracle.clone(),
    )
    .await
    {
        info!(
            "opportunity analysis for {} @ {}: highest profit before TX fees ${} - ({:?})",
            uw_event.address,
            uw_event.trace_id.clone(),
            best_pair.net_profit,
            process_uw_event_timer.elapsed(),
        );
    } else {
        warn!(
            "No profitable liquidation pair found for {}",
            uw_event.address
        );
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    _setup_logging();
    info!("Starting Profito RS");
    let provider_cache = Arc::new(ProviderCache::new());
    let price_cache = Arc::new(Mutex::new(PriceCache::new(3)));
    let context = zmq::Context::new();
    let socket = context.socket(zmq::PULL).unwrap();
    if let Err(e) = socket.bind(PROFITO_INBOUND_ENDPOINT) {
        error!("Failed to bind ZMQ socket: {e}");
        std::process::exit(1);
    }
    info!(
        "Listening for health factor alerts on {}",
        PROFITO_INBOUND_ENDPOINT
    );
    loop {
        match socket.recv_bytes(0) {
            Ok(bytes) => match bincode::deserialize::<UnderwaterUserEvent>(&bytes) {
                Ok(uw_event) => {
                    let provider_cache = provider_cache.clone();
                    let cloned_uw_event = uw_event.clone();

                    // Price cache needs to contain the new prices before processing the event
                    if !price_cache
                        .lock()
                        .await
                        .override_price(cloned_uw_event.trace_id.clone(), cloned_uw_event.new_asset_prices)
                        .await {
                            warn!("Price(s) for uw_event with trace_id {} couldn't be overriden. Next calculations won't consider the pending price update TX values.", uw_event.trace_id);
                        }
                    let price_cache = price_cache.clone();
                    tokio::spawn(async move {
                        if let Err(e) = process_uw_event(
                            uw_event,
                            provider_cache,
                            price_cache,
                        )
                        .await
                        {
                            warn!("Failed to process underwater event: {e}");
                        }
                    });
                }
                Err(e) => warn!("Failed to deserialize message: {e}"),
            },
            Err(e) => warn!("Failed to receive ZMQ message: {e}"),
        }
    }
}
