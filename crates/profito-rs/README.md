# profito-rs

Given an underwater user alert (that is, HF < 1), `profito-rs` aims to answer the question of _is this user profitable to liquidate?_

## Calculation

The most basic sanity check we can make is net profitability, which is composed of 3 parts

$NetProfit = LiquidationBonus - DeterministicCosts - NonDeterministicCosts$

### Liquidation Bonus

`Liquidation Bonus` is what _adds_ to the equation.

$LiquidationBonus = DebtRepaid × BonusMultiplier − DebtRepaid$

- $BonusMultiplier$ is the value of the `liquidation bonus` column of [config.fyi chart](https://www.config.fyi/) (which can be queried from the `AaveProtocolDataProvider` contract using [getReserveConfigurationData](https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/misc/AaveProtocolDataProvider.sol#L77))
- In order to calculate the value of $DebtRepaid$, there are two questions that must be answered first: _which_ reserve do we want to liquidate, and how much of that reserve _can_ we liquidate. The later is determined by the relationship between `HF` and `CLOSE_FACTOR_HF_THRESHOLD`, the former by iterating over all reserves the user has as collateral.

These values can be calculated from Rust land.

### Deterministic Costs

`Deterministic Costs` are the ones we either know for sure how much they're going to eat into our profits, or that we can at least bound somehow.

$DeterministicCosts = GasUsed × (BaseFee + PriorityFee) + (DebtRepaid × FlashLoanRate) + (CollateralToReceive × SlippageRate)$

- $GasUsed$ can be calculated using `eth_estimateGas`
- $BaseFee$ can be calculated using `eth_gasPrice`. When implementing this, refer to [Calculating the Max Fee](https://www.blocknative.com/blog/eip-1559-fees#3) section of that article (which is linked from the official Ethereum docs)
- $DebtRepaid$ is already calculated above
- $FlashLoanRate$ is determined by [FLASHLOAN_PREMIUM_TOTAL](https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/interfaces/IPool.sol#L690). According to [Aave docs](https://aave.com/docs/developers/flash-loans#overview-flash-loan-fee), it's initialized at 0.05% of whatever asset amount you're flash-loaning, but you should query the `Pool` contract to get this value, just in case.
- $CollateralToReceive$ is the amount of collateral you'd receive if the liquidation is executed.
- $SlippageRate$ is a percentage of the $CollateralToReceive$ that you lose when converting back from the awarded collateral back to the base asset.

### NonDeterministic Costs

`NonDeterministic costs` are the ones that we can't easily predict, or that we can't easily know how much we're going to need to make our TX land.

$NonDeterministicCosts = CoinbaseBribe − RefundedETH$

$CoinbaseBribe$ is a value on top of the $PriorityFee$ that goes straight to the builder's _Coinbase_ wallet.
$RefundedETH$ is the amount you can get refunded if you didn't end up using all the gas fees. According to beaverbuild docs, it negatively impacts the prioritization of your TX.

## Open questions

### 1. How do we determine which debt/collateral pair to pass to the `liquidationCall()` function?
_Assuming_ (I'm 99,9% sure, but not 100% sure) that values for each [UserReserveData](https://github.com/aave-dao/aave-v3-origin/blob/ae2d19f998b421b381b85a62d79ecffbb0701501/src/contracts/helpers/interfaces/IUiPoolDataProviderV3.sol#L57-L62) element returned by [getUserReservesData()](https://github.com/aave-dao/aave-v3-origin/blob/main/src/contracts/helpers/UiPoolDataProviderV3.sol#L220C12-L220C31) are denominated in ETH, then the calculations are:

$RawCollateralReceived = DebtRepaid × BonusMultiplier$

$FinalCollateralReceived = RawCollateralReceived × (1 - slippage)$

$NetProfit = FinalCollateralReceived - DebtRepaid - GasCost$



### 2. Why would we ever want to ask for a refund ETH amount? (only saw this on beaver, btw)
TBA

### 3. What exactly is `slippage rate`? Do we even need to account for it?
$SlippageRate$ is already described above, and it's required because you need to account for the full round trip of the asset. Meaning that if you start with ETH, for example, and use it to liquidate a position for which you might get a different asset as reward, you need to conver that asset back to ETH in order to settle that profit (or loss). You can always keep the reward asset, but you expose yourself to fluctuations on it's price.

### 4. If `CLOSE_FACTOR_HF_THRESHOLD` < HF < 1, then only 50% of the debt can be liquidated. If HF < `CLOSE_FACTOR_HF_THRESHOLD`, then 100% of the debt can be liquidated. Is `CLOSE_FACTOR_HF_THRESHOLD` an attribute of the reserve?

No, `CLOSE_FACTOR_HF_THRESHOLD` is hardcoded into the [LiquidationLogic](https://github.com/aave/aave-v3-core/blob/782f51917056a53a2c228701058a6c3fb233684a/contracts/protocol/libraries/logic/LiquidationLogic.sol#L68C27-L68C63) contract, and it's defined as 0.95e18. If the HF is below 1, but above that, then only 50% can be liquidated. If it's under that value, then you can liquidate 100% of whatever debt you choose.
