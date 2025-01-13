use alloy::{
    primitives::{Address, U256},
    sol,
};
use overlord_shared_types::PriceUpdateBundle;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufRead},
    str::FromStr,
};

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    AaveV3Pool,
    "src/abis/aave_v3_pool.json"
);

pub struct HealthFactorCalculationResults {
    pub raw_results: HashMap<Address, U256>,
    pub under_1_hf: HashMap<Address, U256>,
}

pub struct AddressCache {
    addresses: HashMap<Address, U256>,
    buckets: usize,
}

impl AddressCache {
    pub fn new(buckets: usize) -> Self {
        AddressCache {
            addresses: HashMap::new(),
            buckets,
        }
    }

    pub fn load_user_addresses_from_file(&mut self, filepath: &str) -> io::Result<()> {
        let file = File::open(filepath)?;
        for line in io::BufReader::new(file).lines() {
            let address =
                Address::from_str(str::trim(&line.unwrap())).expect("Failed to parse address");
            self.addresses.insert(address, U256::MAX);
        }
        eprintln!("Loaded {} addresses from file", self.addresses.len());
        Ok(())
    }

    /// Returns the user addresses affected by this price update bundle
    pub fn get_user_addresses_for_bundle(&mut self, bundle: Option<&PriceUpdateBundle>) -> Vec<Vec<Address>> {
        if bundle.is_none() {
            return self.bucketize_addresses();
        }
        let last_known_price = U256::from(0);  //TODO(Hernan) get this from cache
        let affected_user_addresses = vec![vec![]];
        if bundle.unwrap().tx_new_price > last_known_price {
            // asset appreciated, check only borrowed assets
        } else if bundle.unwrap().tx_new_price < last_known_price {
            // asset depreciated, check only collateral assets
        }
        affected_user_addresses
    }

    pub fn bucketize_addresses(&mut self) -> Vec<Vec<Address>> {
        let mut buckets = vec![Vec::new(); self.buckets];
        for (i, address) in self.addresses.keys().enumerate() {
            buckets[i % self.buckets].push(*address);
        }
        buckets
    }

    pub fn update(&mut self, addresses_to_keep: HashMap<Address, U256>) -> usize {
        self.addresses = addresses_to_keep;
        self.addresses.len()
    }

    pub fn ignore(&mut self, addresses_to_ignore: HashMap<Address, U256>) -> usize {
        for address in addresses_to_ignore.keys() {
            self.addresses.remove(address);
        }
        self.addresses.len()
    }
}
