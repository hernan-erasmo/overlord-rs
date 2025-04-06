use alloy::{
    primitives::{Address, Bytes, U256},
    sol,
};
use serde::{Deserialize, Serialize};

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(serde::Serialize, serde::Deserialize)]
    #[sol(rpc)]
    AaveV3Pool,
    "src/abis/aave_v3_pool.json"
);

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct UnderwaterUserEvent {
    pub address: Address,
    pub trace_id: String,
    pub total_collateral_base: U256,
    pub user_account_data: AaveV3Pool::getUserAccountDataReturn,
    pub new_asset_prices: Vec<(Address, String, U256)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PriceUpdateBundle {
    pub trace_id: String,       // This is just a string that helps trace the bundle through overlord
    pub tx_new_price: U256,     // The new price of the asset indicated by this update. Used by profito to calculate profit
    pub forward_to: Address,    // Used in chainlink_address_to_asset mapping to determine which asset is affected by this tx
    pub tx_from: Address,       // Used to recreate the price update tx. This is the address that submitted the forward() call.
    pub tx_to: Address,         // Used to recreate the price update tx. This is the address that receives the forward() call.
    pub tx_input: Bytes,        // Used to recreate the price update tx. These are the contents of the forward() call.
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WhistleblowerEventType {
    LiquidationCall,
    Borrow,
    Supply,
    Repay,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WhistleblowerEventDetails {
    pub event: WhistleblowerEventType,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WhistleblowerUpdate {
    pub trace_id: String, // First few chars of tx hash
    pub block_number: u64,
    pub event_details: WhistleblowerEventDetails,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MessageBundle {
    PriceUpdate(PriceUpdateBundle),
    WhistleblowerNotification(WhistleblowerUpdate),
}
