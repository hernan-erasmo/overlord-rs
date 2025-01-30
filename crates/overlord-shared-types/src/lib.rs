use alloy::primitives::{Address, Bytes, U256};
use serde::{Deserialize, Serialize};


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
    pub trace_id: String,  // First few chars of tx hash
    pub block_number: u64,
    pub event_details: WhistleblowerEventDetails,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MessageBundle {
    PriceUpdate(PriceUpdateBundle),
    WhistleblowerNotification(WhistleblowerUpdate),
}
