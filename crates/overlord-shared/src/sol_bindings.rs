use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    ERC20,
    "src/abis/erc20.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    interface IERC20Metadata {
        function symbol() external view returns (string);
    }
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    IAToken,
    "src/abis/iatoken.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    AaveUIPoolDataProvider,
    "src/abis/aave_ui_pool_data_provider.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    AaveOracle,
    "src/abis/aave_v3_oracle.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    AaveProtocolDataProvider,
    "src/abis/aave_protocol_data_provider.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    UniswapV3Quoter,
    "src/abis/uniswap_v3_quoter.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    UniswapV3Factory,
    "src/abis/uniswap_v3_factory.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    UniswapV3Pool,
    "src/abis/uniswap_v3_pool.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    EACAggregatorProxy,
    "src/abis/aggregators/EACAggregatorProxy.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    PriceCapAdapterStable,
    "src/abis/aggregators/PriceCapAdapterStable.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    CLSynchronicityPriceAdapterPegToBase,
    "src/abis/aggregators/CLSynchronicityPriceAdapterPegToBase.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    AccessControlledOCR2Aggregator,
    "src/abis/aggregators/AccessControlledOCR2Aggregator.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    #[derive(Debug)]
    AuthorizedForwarder,
    "src/abis/AuthorizedForwarder.json"
);

pub mod WstETHAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        WstETHPriceCapAdapter,
        "src/abis/aggregators/WstETHPriceCapAdapter.json"
    );
}

pub mod PendlePriceCapAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        PendlePriceCapAdapter,
        "src/abis/aggregators/PendlePriceCapAdapter.json"
    );
}

pub mod CbETHAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        CbETHPriceCapAdapter,
        "src/abis/aggregators/CbETHPriceCapAdapter.json"
    );
}

pub mod RETHAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        RETHPriceCapAdapter,
        "src/abis/aggregators/RETHPriceCapAdapter.json"
    );
}

pub mod EBTCAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        EBTCPriceCapAdapter,
        "src/abis/aggregators/EBTCPriceCapAdapter.json"
    );
}

pub mod WeETHAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        WeETHPriceCapAdapter,
        "src/abis/aggregators/WeETHPriceCapAdapter.json"
    );
}

pub mod OsETHAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        OsETHPriceCapAdapter,
        "src/abis/aggregators/OsETHPriceCapAdapter.json"
    );
}

pub mod EthXAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        EthXPriceCapAdapter,
        "src/abis/aggregators/EthXPriceCapAdapter.json"
    );
}

pub mod SUSDeAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        SUSDePriceCapAdapter,
        "src/abis/aggregators/SUSDePriceCapAdapter.json"
    );
}

pub mod sDAIAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        sDAISynchronicityPriceAdapter,
        "src/abis/aggregators/sDAISynchronicityPriceAdapter.json"
    );
}

pub mod RsETHAggregator {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[sol(rpc)]
        #[derive(Debug)]
        RsETHPriceCapAdapter,
        "src/abis/aggregators/RsETHPriceCapAdapter.json"
    );
}

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug, PartialEq)]
    #[sol(rpc)]
    contract Foxdie {
        enum FlashLoanSource {
            NONE,      // 0 - invalid
            MORPHO,    // 1
            AAVE_V3,   // 2
            BALANCER   // 3
        }

        struct LiquidationParams {
            uint256 debtAmount;    // Amount to repay
            address user;           // User to liquidate
            address debtAsset;     // Asset they borrowed
            address collateral;     // Their collateral asset
            uint24 collateralToWethFee; // Uniswap pool fee tier
            uint24 wethToDebtFee;       // Uniswap pool fee tier
            uint16 bribePercentBps;    // Builder bribe in basis points (e.g., 1500 = 15%)
            FlashLoanSource flashLoanSource; // Which protocol to use for flash loan
            uint256 aavePremium; // AAVE V3 premium (if applicable, otherwise zero)
        }

        function triggerLiquidation(LiquidationParams calldata params) external;
    }
);

pub mod pool {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[derive(serde::Serialize, serde::Deserialize)]
        #[sol(rpc)]
        AaveV3Pool,
        "src/abis/aave_v3_pool.json"
    );
}

pub type GetReserveConfigurationDataReturn =
    AaveProtocolDataProvider::getReserveConfigurationDataReturn;
