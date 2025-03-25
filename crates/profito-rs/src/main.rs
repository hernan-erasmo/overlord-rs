mod cache;
mod calculations;
mod constants;
pub mod sol_bindings;
mod utils;

use alloy::{providers::RootProvider, pubsub::PubSubFrontend};
use cache::{PriceCache, ProviderCache};
use calculations::get_best_debt_collateral_pair;
use constants::*;
use overlord_shared_types::UnderwaterUserEvent;
use sol_bindings::{AaveOracle, AaveUIPoolDataProvider};
use std::{sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};
use utils::{generate_reserve_details_by_asset, ReserveConfigurationData};

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
    reserves_configuration: ReserveConfigurationData,
    provider_cache: Arc<ProviderCache>,
    price_cache: Arc<tokio::sync::Mutex<PriceCache>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let process_uw_event_timer = Instant::now();
    match provider_cache.get_provider().await {
        Ok(provider) => {
            let ui_data = AaveUIPoolDataProvider::new(
                AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS,
                provider.clone(),
            );
            let aave_oracle: AaveOracle::AaveOracleInstance<
                PubSubFrontend,
                Arc<RootProvider<PubSubFrontend>>,
            > = AaveOracle::new(AAVE_ORACLE_ADDRESS, provider.clone());

            match ui_data
                .getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, uw_event.address)
                .call()
                .await
            {
                Ok(user_reserves_data) => {
                    if !price_cache
                        .lock()
                        .await
                        .override_price(uw_event.trace_id.clone(), uw_event.new_asset_prices)
                        .await
                    {
                        warn!("Price(s) for uw_event with trace_id {} couldn't be overriden. Next calculations won't consider the pending price update TX values.", uw_event.trace_id);
                    }
                    if let Some(best_pair) = get_best_debt_collateral_pair(
                        uw_event.address,
                        reserves_configuration,
                        user_reserves_data._0,
                        uw_event.user_account_data.healthFactor,
                        price_cache,
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
                }
                Err(e) => {
                    warn!("Failed to fetch user reserves data: {e}");
                }
            }
        }
        Err(e) => warn!("Failed to get the provider for uw processing: {e}"),
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
    let reserves_configuration = generate_reserve_details_by_asset(provider_cache.get_provider().await.unwrap())
        .await
        .unwrap_or_else(|e| {
            error!("Failed to initialize reserve configuration: {}", e);
            std::process::exit(1);
        });
    loop {
        match socket.recv_bytes(0) {
            Ok(bytes) => match bincode::deserialize::<UnderwaterUserEvent>(&bytes) {
                Ok(uw_event) => {
                    let reserves_configuration = reserves_configuration.clone();
                    let provider_cache = provider_cache.clone();
                    let price_cache = price_cache.clone();
                    tokio::spawn(async move {
                        if let Err(e) = process_uw_event(
                            uw_event,
                            reserves_configuration,
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
