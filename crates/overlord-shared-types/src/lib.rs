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
    pub user_account_data: AaveV3Pool::getUserAccountDataReturn,
    pub new_asset_prices: Vec<(Address, String, U256)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PriceUpdateBundle {
    pub trace_id: String,
    pub tx_new_price: U256,
    pub forward_to: Address,
    pub tx_from: Address,
    pub tx_to: Address,
    pub tx_input: Bytes,
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
