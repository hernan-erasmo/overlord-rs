use alloy::{
    primitives::{Address, U256},
    providers::{IpcConnect, ProviderBuilder},
};
use std::fs::File;
use std::io::Write;
use bincode::deserialize;
use chrono::Local;
use clap::Parser;
use tokio::time::Instant;
use overlord_shared_types::{
    MessageBundle,
    PriceUpdateBundle,
};
use std::error::Error;
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};
use vega_rs::user_reserve_cache::UserReservesCache;
use vega_rs::calc_utils::get_hf_for_users;
use vega_rs::fork_provider::ForkProvider;

const VEGA_INBOUND_ENDPOINT: &str = "ipc:///tmp/vega_inbound";

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

    #[clap(long, default_value = "addresses.txt")]
    addresses_file: String,

    #[clap(long, default_value = "asset_to_contract_address_mapping.csv")]
    chainlink_addresses_file: String,
}

async fn run_price_update_pipeline(
    cache: &mut UserReservesCache,
    bundle: Option<&PriceUpdateBundle>,
) {
    let pipeline_processing = Instant::now();
    let address_buckets = cache.get_candidates_for_bundle(bundle).await;
    if address_buckets.len() == 1 && address_buckets[0].is_empty() {
        return;
    }
    let fork_provider = match ForkProvider::new(bundle).await {
        Ok(provider) => provider,
        Err(e) => {
            warn!("Failed to spin up fork: {:?}", e);
            return;
        }
    };
    let trace_id = bundle.map_or("initial-run".to_string(), |b| b.trace_id.clone());
    let trace_id_clone = trace_id.clone();
    let alert_callback = move |address: Address, hf: U256, collateral: U256| {
        info!(
            "ALERT | {} | {} has HF < 1: {} (total collateral {})",
            trace_id_clone, address, hf, collateral
        );
    };
    let results = get_hf_for_users(
        address_buckets,
        fork_provider.fork_provider.as_ref().unwrap(),
        Some(alert_callback),
    )
    .await;
    let pipeline_processing_elapsed = pipeline_processing.elapsed().as_millis();
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    info!(
        "{} | pipeline:{} results | {} ms | {} candidates processed | {} with HF < 1",
        now,
        trace_id.clone(),
        pipeline_processing_elapsed,
        results.raw_results.len(),
        results.under_1_hf.len()
    );
    let hf_traces_filepath = format!("hf-traces/{}.txt", trace_id);
    let hf_traces_file = match File::create(hf_traces_filepath.clone()) {
        Ok(file) => file,
        Err(e) => {
            warn!("Failed to create HF traces file {}: {}", hf_traces_filepath, e);
            return;
        }
    };
    let mut hf_traces_file = hf_traces_file;
    for (address, hf) in results.raw_results.iter() {
        if let Err(e) = writeln!(hf_traces_file, "{:?} {}", address, hf) {
            warn!("Failed to write to HF traces file {}: {}", hf_traces_filepath, e);
            return;
        }
    }
}

async fn _dump_initial_hf_results(user_buckets: Vec<Vec<Address>>) -> Result<(), Box<dyn Error>> {
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
    let init_hf_results = get_hf_for_users(
        user_buckets,
        &provider,
        None::<fn(Address, U256, U256)>
    ).await;
    let init_hf_results_filepath = format!("init_hf_under_1_results_{}.txt", Local::now().format("%Y%m%d"));
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
    info!(filepath = init_hf_results_filepath, elapsed_ms = init_hf_results_elapsed, "Initial HF results dumped");
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
    let mut user_reserves_cache = UserReservesCache::new();
    let user_buckets = match user_reserves_cache.initialize_cache(&args.addresses_file, &args.chainlink_addresses_file).await {
        Ok(buckets) => buckets,
        Err(e) => {
            error!("Failed to initialize cache: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = _dump_initial_hf_results(user_buckets).await {
        error!("Failed to dump initial HF results: {:?}", e);
        std::process::exit(1);
    }

    // Create IPC file and start listening for price updates
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
    loop {
        info!("VEGA is running and listening for price updates...");
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
                run_price_update_pipeline(&mut user_reserves_cache, Some(&price_update)).await;
            }
            MessageBundle::WhistleblowerNotification(whistleblower_update) => {
                info!(update_details = ?whistleblower_update, "Received whistleblower update");
                user_reserves_cache.update_cache(&whistleblower_update).await;
            }
        };
    }
}
