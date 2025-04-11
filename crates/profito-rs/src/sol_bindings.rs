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
        #[sol(rpc)]
        AaveV3Pool,
        "src/abis/aave_v3_pool.json"
    );
}

pub type GetReserveConfigurationDataReturn =
    AaveProtocolDataProvider::getReserveConfigurationDataReturn;
