use alloy::{
    primitives::{Address, U256},
    providers::{IpcConnect, ProviderBuilder},
};
use bincode::deserialize;
use chrono::Local;
use clap::Parser;
use overlord_shared_types::{MessageBundle, PriceUpdateBundle};
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tokio::time::Instant;
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};
use vega_rs::calc_utils::{get_hf_for_users, UnderwaterUserEventBus};
use vega_rs::fork_provider::ForkProvider;
use vega_rs::user_reserve_cache::UserReservesCache;

const VEGA_INBOUND_ENDPOINT: &str = "ipc:///tmp/vega_inbound";
const PROFITO_INBOUND_ENDPOINT: &str = "ipc:///tmp/profito_inbound";
const ADDRESSES_FILE_ENV: &str = "VEGA_ADDRESSES_FILE";
const CHAINLINK_ADDRESSES_FILE_ENV: &str = "VEGA_CHAINLINK_ADDRESSES_FILE";
const TEMP_OUTPUT_DIR: &str = "TEMP_OUTPUT_DIR";

#[derive(Parser)]
#[clap(
    name = "vega-rs",
    version = "1.0",
    author = "hernan",
    about = "Vega listen's for transactions and does math"
)]
struct VegaArgs {
    #[clap(long, default_value = "64")]
    buckets: usize,
}

fn get_required_env_var(key: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(key).map_err(|e| {
        error!("Environment variable {} not set: {}", key, e);
        Box::new(e) as Box<dyn std::error::Error>
    })
}

async fn run_price_update_pipeline(
    cache: &mut UserReservesCache,
    bundle: Option<&PriceUpdateBundle>,
    output_data_dir: &str,
    event_bus: Arc<UnderwaterUserEventBus>,
) {
    let pipeline_processing = Instant::now();
    let (address_buckets, affected_reserves) = cache.get_candidates_for_bundle(bundle).await;
    let trace_id = bundle.map_or("initial-run".to_string(), |b| b.trace_id.clone());
    if address_buckets.len() == 1 && address_buckets[0].is_empty() {
        info!("Not processing bundle for trace_id {} because it doesn't contain any addresses", trace_id);
        return;
    }
    let fork_provider = match ForkProvider::new(bundle).await {
        Ok(provider) => provider,
        Err(e) => {
            warn!("Failed to spin up fork for bundle {}: {:?}", trace_id, e);
            return;
        }
    };
    let new_prices_by_asset = affected_reserves
        .iter()
        .map(|r_info| {
            (
                r_info.reserve_address,
                r_info.symbol.clone(),
                bundle.map_or(U256::ZERO, |b| b.tx_new_price),
            )
        })
        .collect::<Vec<(Address, String, U256)>>();
    let results = get_hf_for_users(
        address_buckets,
        fork_provider.fork_provider.as_ref().unwrap(),
        Some(trace_id.clone()),
        new_prices_by_asset,
        Some(event_bus),
    )
    .await;
    let pipeline_processing_elapsed = pipeline_processing.elapsed().as_millis();
    info!(
        "Candidates analysis complete for {} | {} ms | {} candidates processed | {} with HF < 1",
        trace_id.clone(),
        pipeline_processing_elapsed,
        results.raw_results.len(),
        results.under_1_hf.len()
    );
    let hf_traces_dir = format!("{}/hf-traces", output_data_dir);
    if !std::path::Path::new(output_data_dir).is_dir() {
        error!("Output directory does not exist for bundle {}: {}", trace_id, output_data_dir);
        return;
    }
    std::fs::create_dir_all(&hf_traces_dir)
        .map_err(|e| {
            error!("Failed to create hf-traces directory for bundle {}: {}", trace_id, e);
        })
        .ok();
    let hf_traces_filepath = format!("{}/{}.txt", hf_traces_dir, trace_id);
    let hf_traces_file = match File::create(hf_traces_filepath.clone()) {
        Ok(file) => file,
        Err(e) => {
            warn!(
                "Failed to create HF traces file {}: {}",
                hf_traces_filepath, e
            );
            return;
        }
    };
    let mut hf_traces_file = hf_traces_file;
    for (address, hf) in results.raw_results.iter() {
        if let Err(e) = writeln!(hf_traces_file, "{:?} {}", address, hf) {
            warn!(
                "Failed to write to HF traces file {}: {}",
                hf_traces_filepath, e
            );
            return;
        }
    }
}

async fn _dump_initial_hf_results(
    user_buckets: Vec<Vec<Address>>,
    output_data_dir: &str,
    event_bus: Arc<UnderwaterUserEventBus>,
) -> Result<(), Box<dyn Error>> {
    let init_hf_results_timer = Instant::now();
    let ipc_url = "/tmp/reth.ipc";
    let ipc = IpcConnect::new(ipc_url.to_string());
    let provider = match ProviderBuilder::new().on_ipc(ipc).await {
        Ok(provider) => provider,
        Err(e) => {
            error!("Failed to connect to IPC: {}", e);
            return Err(Box::new(e));
        }
    };
    let init_hf_results =
        get_hf_for_users(user_buckets, &provider, None, vec![], Some(event_bus)).await;
    let init_hf_results_filepath = format!(
        "{}/init_hf_under_1_results_{}.txt",
        output_data_dir,
        Local::now().format("%Y%m%d")
    );

    // Check if directory exists
    if !std::path::Path::new(output_data_dir).is_dir() {
        error!("Output directory does not exist: {}", output_data_dir);
        return Err("Output directory does not exist".into());
    }

    let init_hf_results_file = match File::create(init_hf_results_filepath.clone()) {
        Ok(file) => file,
        Err(e) => {
            error!("Failed to create init HF results file: {}", e);
            return Err(Box::new(e));
        }
    };
    let mut init_hf_results_file = init_hf_results_file;
    for (address, hf) in init_hf_results.under_1_hf.iter() {
        if let Err(e) = writeln!(init_hf_results_file, "{:?}: {}", address, hf) {
            error!("Failed to write to init HF results file: {}", e);
            return Err(Box::new(e));
        }
    }
    let init_hf_results_elapsed = init_hf_results_timer.elapsed().as_millis();
    info!(
        filepath = init_hf_results_filepath,
        elapsed_ms = init_hf_results_elapsed,
        "Initial HF results dumped"
    );
    Ok(())
}

fn _setup_logging() {
    let log_file =
        rolling::RollingFileAppender::new(Rotation::DAILY, "/var/log/overlord-rs", "vega-rs.log");
    let file_writer = BoxMakeWriter::new(log_file);
    tracing_subscriber::fmt()
        .with_writer(file_writer)
        .with_timer(LocalTime::rfc_3339())
        .with_target(true)
        .init();
}

#[tokio::main]
async fn main() -> Result<(), String> {
    _setup_logging();

    let args = VegaArgs::parse();

    info!(buckets = args.buckets, "vega-rs starting");
    let addresses_file = match get_required_env_var(ADDRESSES_FILE_ENV) {
        Ok(filename) => filename,
        Err(e) => {
            error!("Failed to get addresses file path: {}", e);
            std::process::exit(1);
        }
    };
    let chainlink_addresses_file = match get_required_env_var(CHAINLINK_ADDRESSES_FILE_ENV) {
        Ok(filename) => filename,
        Err(e) => {
            error!("Failed to get chainlink addresses file path: {}", e);
            std::process::exit(1);
        }
    };
    let temp_output_dir = match get_required_env_var(TEMP_OUTPUT_DIR) {
        Ok(pathname) => pathname,
        Err(e) => {
            error!(
                "Failed to get TEMP_OUTPUT_DIR path from environment variable: {}",
                e
            );
            std::process::exit(1);
        }
    };
    let mut user_reserves_cache = UserReservesCache::new();
    let user_buckets = match user_reserves_cache
        .initialize_cache(&addresses_file, &chainlink_addresses_file, &temp_output_dir)
        .await
    {
        Ok(buckets) => buckets,
        Err(e) => {
            error!("Failed to initialize cache: {}", e);
            std::process::exit(1);
        }
    };

    let uw_event_bus = Arc::new(UnderwaterUserEventBus::new(10000));
    let mut uw_log_subscriber = uw_event_bus.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = uw_log_subscriber.recv().await {
            info!(
                "ALERT (from event bus) | {} | {} has HF < 1: {} (total collateral {})",
                event.trace_id,
                event.address,
                event.user_account_data.healthFactor,
                event.user_account_data.totalCollateralBase
            );
        }
    });
    let mut profito_subscriber = uw_event_bus.subscribe();
    tokio::spawn(async move {
        let context = zmq::Context::new();
        let profito_socket = context.socket(zmq::PUSH).unwrap();
        if let Err(e) = profito_socket.connect(PROFITO_INBOUND_ENDPOINT) {
            error!("Failed to connect to profito-rs: {}", e);
            return;
        }
        while let Ok(event) = profito_subscriber.recv().await {
            if let Ok(bytes) = bincode::serialize(&event) {
                if let Err(e) = profito_socket.send(&bytes, 0) {
                    error!("Failed to send message to profito-rs: {}", e);
                }
            }
        }
    });

    if let Err(e) =
        _dump_initial_hf_results(user_buckets, &temp_output_dir, uw_event_bus.clone()).await
    {
        error!("Failed to dump initial HF results: {:?}", e);
        std::process::exit(1);
    }

    // Create IPC file and start listening for price updates
    info!("Setting up vega-rs ZMQ socket for inbound connections...");
    let context = zmq::Context::new();
    let inbound_socket = match context.socket(zmq::PULL) {
        Ok(socket) => socket,
        Err(e) => {
            error!("Failed to create ZMQ socket: {}", e);
            std::process::exit(1);
        }
    };

    match inbound_socket.bind(VEGA_INBOUND_ENDPOINT) {
        Ok(_) => info!("Successfully bound to {}", VEGA_INBOUND_ENDPOINT),
        Err(e) => {
            error!("Failed to bind socket to {}: {}", VEGA_INBOUND_ENDPOINT, e);
            std::process::exit(1);
        }
    };
    info!("VEGA is running and listening for price updates...");
    loop {
        let msg = match inbound_socket.recv_bytes(0) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("Failed to receive inbound update: {}", e);
                continue;
            }
        };
        let deserialized_message = match deserialize::<MessageBundle>(&msg) {
            Ok(message) => message,
            Err(e) => {
                warn!("Failed to deserialize inbound update: {}", e);
                continue;
            }
        };
        match deserialized_message {
            MessageBundle::PriceUpdate(price_update) => {
                run_price_update_pipeline(
                    &mut user_reserves_cache,
                    Some(&price_update),
                    &temp_output_dir,
                    uw_event_bus.clone(),
                )
                .await;
            }
            MessageBundle::WhistleblowerNotification(whistleblower_update) => {
                info!(update_details = ?whistleblower_update, "Received whistleblower update");
                if let Err(e) = user_reserves_cache
                    .update_cache(&whistleblower_update)
                    .await
                {
                    warn!("Failed to update cache: {}", e);
                }
            }
        };
    }
}
