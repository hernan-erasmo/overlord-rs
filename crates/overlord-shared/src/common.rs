use alloy::{providers::RootProvider, pubsub::PubSubFrontend};

use std::sync::Arc;

use crate::constants::{AAVE_V3_PROVIDER_ADDRESS, AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS};
use crate::sol_bindings::{AaveUIPoolDataProvider, IUiPoolDataProviderV3::AggregatedReserveData};

pub async fn get_reserves_data(
    provider: Arc<RootProvider<PubSubFrontend>>,
) -> Result<Vec<AggregatedReserveData>, Box<dyn std::error::Error>> {
    /*
       According to https://github.com/aave-dao/aave-v3-origin/blob/a0512f8354e97844a3ed819cf4a9a663115b8e20/src/contracts/helpers/UiPoolDataProviderV3.sol#L45
       the reserves data is ordered the same way as the reserves list (it actually calls pool.getReservesList() and uses it as index)
    */
    match AaveUIPoolDataProvider::new(AAVE_V3_UI_POOL_DATA_PROVIDER_ADDRESS, provider.clone())
        .getReservesData(AAVE_V3_PROVIDER_ADDRESS)
        .call()
        .await
    {
        Ok(reserves_data) => Ok(reserves_data._0),
        Err(e) => Err(format!("Error trying to call getReservesData: {}", e).into()),
    }
}
