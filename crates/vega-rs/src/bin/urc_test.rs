//! This is a self-contained prototype that creates a data structure called `user_by_position_by_asset`,
//! which has the following shape:
//! 
//! 
//! {
//! "0x5f98805A4E8be255a32880FDeC7F6728C6568bA0": {
//!		"Position.BORROW": [
//!			"0x_user1",
//!			"0x_user2",
//!			"0x_user3",
//!			"0x_user4",
//!			...
//!		],
//!		"Position.COLLATERAL": [
//!			"0x_user1",
//!			"0x_user3",
//!		]
//!	},
//!	"0x111111111117dC0aa78b770fA6A738034120C302": {
//!		"Position.BORROW": [
//!			"0x_user1",
//!			"0x_user4",
//!			...
//!		],
//!		"Position.COLLATERAL": [
//!			"0x_user3",
//!		]
//!	},
//!	...
//!}
//! 
//! The goal of this structure is to optimize price update calculations by reducing the candidate address
//! space.
//! 
//! The main function orchestrates the entire process, from loading addresses to generating the final
//! data structure.
//! 

use alloy::{
    primitives::{address, Address, U256},
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
    sol,
};
use futures::future::join_all;
use std::{
    collections::HashMap, error::Error, fs::File, io::{self, BufRead}, str::FromStr
};
use tokio::{task, time::Instant};
use clap::Parser;

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
}

/*
async fn run_price_update_pipeline(
    cache: &mut UserReserveCache,
    bundle: Option<&PriceUpdateBundle>,
    ignore_hf: bool,
) {
    let pipeline_processing = Instant::now();
    let fork_provider = ForkProvider::new(bundle).await;
    let address_buckets = cache.get_user_addresses_for_bundle(bundle);
    let trace_id = bundle.map_or("initial-run".to_string(), |b| b.trace_id.clone());
    let results = get_hf_for_users(
        address_buckets,
        fork_provider.fork_provider.unwrap(),
        trace_id.clone(),
    )
    .await;
    let pipeline_processing_elapsed = pipeline_processing.elapsed().as_millis();
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    println!(
        "{} | pipeline:{} results | {} ms | {} addresses updated | {} with HF < 1",
        now,
        trace_id.clone(),
        pipeline_processing_elapsed,
        results.raw_results.len(),
        results.under_1_hf.len()
    );
    cache.update(results.raw_results);
    if ignore_hf {
        /* This is because when VEGA first runs, it scans all user addresses for underwater positions. There are
        a few that will be returned on that first sweep, but these are not profitable to liquidate. This function
        runs with ignore_hf = True when the app first starts and removes these addresses from the cache, so that
        subsequent runs only return truly new underwater positions. There are two GOTCHAS to keep in mind about
        this way of doing things:

        1. Underwater positions discovered after running this for the first time might also not be liquidateable,
        and in that case, they'll continue popping up as possible liquidations unless a filtering mechanism is
        implemented downstream

        2. We're assuming these addresses are inactive, so that they won't be borrowing anymore and thus not generate
        valid liquidation opportunities in the future. Once ignored here, there's currently no mechanism in place
        to track updates on them.
        */
        cache.ignore(results.under_1_hf);
    }
}
 */


const AAVE_V3_PROVIDER_ADDRESS: Address = address!("2f39d218133afab8f2b819b1066c7e434ad94e9e");
const AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS: Address = address!("3f78bbd206e4d3c504eb854232eda7e47e9fd8fc");

type UserAddress = Address;
type ReserveAddress = Address;

#[derive(Debug, Eq, PartialEq, Hash)]
enum PositionType {
    BORROWED,
    COLLATERAL,
}

#[derive(Debug)]
struct UserPosition {
    scaled_atoken_balance: U256,
    usage_as_collateral_enabled_on_user: bool,
    scaled_variable_debt: U256,
    underlying_asset: ReserveAddress,
}

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    AaveUIPoolDataProvider,
    "src/abis/aave_ui_pool_data_provider.json"
);

fn load_addresses_from_file(filepath: &str) -> Result<Vec<UserAddress>, Box<dyn Error>> {
    let mut addresses: Vec<UserAddress> = Vec::new();
    let file = File::open(filepath)?;
    for line in io::BufReader::new(file).lines() {
        let address = Address::from_str(str::trim(&line.unwrap())).expect("Failed to parse address");
        addresses.push(address);
    }
    eprintln!("Loaded {} user addresses.", addresses.len());
    Ok(addresses)
}

async fn get_positions_by_user(
    address_buckets: &Vec<Vec<UserAddress>>,
    provider: &RootProvider<PubSubFrontend>,
) -> Result<HashMap<UserAddress, Vec<UserPosition>>, Box<dyn Error>> {
    let mut tasks = vec![];
    let ui_data = AaveUIPoolDataProvider::new(
        AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, 
        provider.clone()
    );
    for bucket in address_buckets.clone() {
        let ui_data = ui_data.clone();
        let task = task::spawn(async move {
            let mut results: HashMap<UserAddress, Vec<UserPosition>> = HashMap::new();
            for address in bucket {
                // returns (UserReserveData[] memory, uint8)
                let result = ui_data.getUserReservesData(AAVE_V3_PROVIDER_ADDRESS, address.clone()).call().await;
                match result {
                    Ok(data) => {
                        let user_positions: Vec<UserPosition> = data._0.iter()
                            .filter(|d| d.scaledVariableDebt > U256::ZERO)
                            .map(|d| UserPosition {
                                scaled_atoken_balance: d.scaledATokenBalance,
                                usage_as_collateral_enabled_on_user: d.usageAsCollateralEnabledOnUser,
                                scaled_variable_debt: d.scaledVariableDebt,
                                underlying_asset: d.underlyingAsset,
                            }).collect();
                        if !user_positions.is_empty() {
                            results.insert(address.clone(), user_positions);
                        }
                    }
                    Err(e) => eprintln!("Couldn't calculate address reserves: {:?}", e),
                }
            }
            results
        });
        tasks.push(task);
    }
    let aggregate_results: Vec<HashMap<UserAddress, Vec<UserPosition>>> = join_all(tasks)
        .await
        .into_iter()
        .filter_map(|bucket| bucket.ok())
        .collect();
    let mut raw_results = HashMap::new();
    for result_bucket in aggregate_results {
        raw_results.extend(result_bucket);
    }
    Ok(raw_results)
}

fn generate_user_by_position_by_asset(positions_by_user: HashMap<UserAddress, Vec<UserPosition>>) -> HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>> {
    let mut user_by_position_by_asset: HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>> = HashMap::new();
    for (user_address, positions) in positions_by_user.iter() {
        for position in positions {
            // if the asset already exists, get it. Otherwise create an empty map for it
            let users_by_position = user_by_position_by_asset.entry(position.underlying_asset.clone()).or_insert_with(HashMap::new);
            
            if position.scaled_variable_debt > U256::ZERO {
                // if the user has debt for that asset, add it to the borrowed vector or create a new empty one and then add it
                let users_vector = users_by_position.entry(PositionType::BORROWED).or_insert_with(Vec::new);
                users_vector.push(user_address.clone());
            }

            if position.usage_as_collateral_enabled_on_user && position.scaled_atoken_balance > U256::ZERO {
                // if the user has balance for that asset (and can be used as collateral), add it to the collateral vector or create a new empty one and then add it
                let users_vector = users_by_position.entry(PositionType::COLLATERAL).or_insert_with(Vec::new);
                users_vector.push(user_address.clone());
            }
        }
    }
    user_by_position_by_asset
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = VegaArgs::parse();
    let total_time = Instant::now();
    eprintln!("Running with {} buckets", args.buckets);
    
    // Step 1: Load and prepare user addresses
    let user_addresses: Vec<UserAddress> = load_addresses_from_file(&args.addresses_file).expect("Failed to load addresses from file");
    let user_addresses_buckets: Vec<Vec<UserAddress>> = user_addresses.chunks(args.buckets).map(|chunk| chunk.to_vec()).collect();

    // Step 2: Setup the provider
    let ipc_path = "/tmp/reth.ipc";
    let ipc = IpcConnect::new(ipc_path.to_string());    
    let provider = ProviderBuilder::new().on_ipc(ipc).await?;

    // Step 3: Get information about user positions
    let positions_by_user_time = Instant::now();
    let positions_by_user: HashMap<UserAddress, Vec<UserPosition>> = get_positions_by_user(&user_addresses_buckets, &provider).await.expect("Failed to get positions by user");
    let elapsed_positions_by_user_time = positions_by_user_time.elapsed().as_millis();

    // Step 4: Re-arrange the information into users by position by asset
    let user_by_position_by_asset_time = Instant::now();
    let user_by_position_by_asset: HashMap<ReserveAddress, HashMap<PositionType, Vec<UserAddress>>> = generate_user_by_position_by_asset(positions_by_user);
    let elapsed_user_by_position_by_asset_time = user_by_position_by_asset_time.elapsed().as_millis();


    /*
    All things below this line are just debugging and extras. The important stuff is above, where we go from zero to
    a data structure capable of greatly optimizing the price update calculations: `user_by_position_by_asset`

    In the actual app, we could start collecting oops-rs messages here.

    The next step is to adapt this so that we can run the price updates against the relevant addresses
    */


    // just for debugging purposes, print the whole user_by_position_by_asset hashmap but only print the first 2 users at most for each pair of
    // position type and asset
    let mut most_borrowed: (String, usize) = (String::new(), 0);
    let mut most_collateral: (String, usize) = (String::new(), 0);
    for (asset, users_by_position) in user_by_position_by_asset.iter() {
        let borrowed_for_asset = users_by_position.get(&PositionType::BORROWED).unwrap_or(&vec![]).len();
        let used_as_collateral = users_by_position.get(&PositionType::COLLATERAL).unwrap_or(&vec![]).len();
        if borrowed_for_asset > most_borrowed.1 {
            most_borrowed = (asset.to_string().clone(), borrowed_for_asset);
        }
        if used_as_collateral > most_collateral.1 {
            most_collateral = (asset.to_string().clone(), used_as_collateral);
        }
        eprintln!("Asset: {} (totals: {} borrowed, {} collateral", asset, borrowed_for_asset, used_as_collateral);
        for (position_type, users) in users_by_position.iter() {
            eprintln!("  Position type: {:?}", position_type);
            for user in users.iter().take(2) {
                eprintln!("    User: {}", user);
            }
        }
    }

    let elapsed_total_time = total_time.elapsed().as_millis();
    eprintln!("Most borrowed asset: {} with {} users", most_borrowed.0, most_borrowed.1);
    eprintln!("Most collateralized asset: {} with {} users", most_collateral.0, most_collateral.1);
    eprintln!("positions_by_user time: {}", elapsed_positions_by_user_time);
    eprintln!("user_by_positions_by_asset time: {}", elapsed_user_by_position_by_asset_time);
    eprintln!("total time: {}", elapsed_total_time);

    Ok(())
}
