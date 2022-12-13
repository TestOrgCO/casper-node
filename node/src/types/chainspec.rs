//! The chainspec is a set of configuration options for the network.  All validators must apply the
//! same set of options in order to join and act as a peer in a given network.

mod accounts_config;
mod activation_point;
mod chainspec_raw_bytes;
mod core_config;
mod deploy_config;
mod error;
mod global_state_update;
mod highway_config;
mod network_config;
mod parse_toml;
mod protocol_config;

use std::{fmt::Debug, path::Path, sync::Arc};

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::Serialize;
use tracing::{error, warn};

use casper_execution_engine::{
    core::engine_state::{genesis::ExecConfig, ChainspecRegistry, UpgradeConfig},
    shared::{system_config::SystemConfig, wasm_config::WasmConfig},
};
use casper_hashing::{ChunkWithProof, Digest};
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    EraId, ProtocolVersion, PublicKey,
};

#[cfg(test)]
pub(crate) use self::accounts_config::{AccountConfig, ValidatorConfig};
pub use self::error::Error;
pub(crate) use self::{
    accounts_config::AccountsConfig,
    activation_point::ActivationPoint,
    chainspec_raw_bytes::ChainspecRawBytes,
    core_config::{ConsensusProtocolName, CoreConfig},
    deploy_config::DeployConfig,
    global_state_update::GlobalStateUpdate,
    highway_config::HighwayConfig,
    network_config::NetworkConfig,
    protocol_config::ProtocolConfig,
};
use crate::utils::Loadable;

/// The name of the chainspec file on disk.
pub const CHAINSPEC_FILENAME: &str = "chainspec.toml";

/// A collection of configuration settings describing the state of the system at genesis and after
/// upgrades to basic system functionality occurring after genesis.
#[derive(DataSize, PartialEq, Eq, Serialize, Debug)]
pub struct Chainspec {
    #[serde(rename = "protocol")]
    pub(crate) protocol_config: ProtocolConfig,
    #[serde(rename = "network")]
    pub(crate) network_config: NetworkConfig,
    #[serde(rename = "core")]
    pub(crate) core_config: CoreConfig,
    #[serde(rename = "highway")]
    pub(crate) highway_config: HighwayConfig,
    #[serde(rename = "deploys")]
    pub(crate) deploy_config: DeployConfig,
    #[serde(rename = "wasm")]
    pub(crate) wasm_config: WasmConfig,
    #[serde(rename = "system_costs")]
    pub(crate) system_costs_config: SystemConfig,
}

impl Chainspec {
    /// Returns `false` and logs errors if the values set in the config don't make sense.
    pub(crate) fn is_valid(&self) -> bool {
        if (self.network_config.maximum_net_message_size as usize)
            < ChunkWithProof::CHUNK_SIZE_BYTES * 3
        {
            warn!(
                "config value [network][maximum_net_message_size] should be set to at least
            CHUNK_SIZE_BYTES * 3 ({})",
                ChunkWithProof::CHUNK_SIZE_BYTES * 3
            );
        }

        if self.core_config.unbonding_delay <= self.core_config.auction_delay {
            warn!(
                "unbonding delay is set to {} but it should be greater than the auction delay (currently set to {})",
                self.core_config.unbonding_delay, self.core_config.auction_delay);
            return false;
        }

        // If the era duration is set to zero, we will treat it as explicitly stating that eras
        // should be defined by height only.
        if self.core_config.era_duration.millis() > 0
            && self.core_config.era_duration
                < self.core_config.minimum_block_time * self.core_config.minimum_era_height
        {
            warn!("era duration is less than minimum era height * block time!");
        }

        if self.core_config.consensus_protocol == ConsensusProtocolName::Highway {
            if self.core_config.minimum_block_time > self.highway_config.maximum_round_length {
                error!(
                    minimum_block_time = %self.core_config.minimum_block_time,
                    maximum_round_length = %self.highway_config.maximum_round_length,
                    "minimum_block_time must be less or equal than maximum_round_length",
                );
                return false;
            }
            if !self.highway_config.is_valid() {
                return false;
            }
        }

        self.protocol_config.is_valid() && self.core_config.is_valid()
    }

    /// Serializes `self` and hashes the resulting bytes.
    pub(crate) fn hash(&self) -> Digest {
        let serialized_chainspec = self.to_bytes().unwrap_or_else(|error| {
            error!(%error, "failed to serialize chainspec");
            vec![]
        });
        Digest::hash(&serialized_chainspec)
    }

    /// Returns the protocol version of the chainspec.
    pub(crate) fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_config.version
    }

    /// Returns the era ID of where we should reset back to.  This means stored blocks in that and
    /// subsequent eras are deleted from storage.
    pub(crate) fn hard_reset_to_start_of_era(&self) -> Option<EraId> {
        self.protocol_config
            .hard_reset
            .then(|| self.protocol_config.activation_point.era_id())
    }

    pub(crate) fn ee_upgrade_config(
        &self,
        pre_state_hash: Digest,
        current_protocol_version: ProtocolVersion,
        era_id: EraId,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
    ) -> Result<UpgradeConfig, String> {
        let chainspec_registry = ChainspecRegistry::new_with_optional_global_state(
            chainspec_raw_bytes.chainspec_bytes(),
            chainspec_raw_bytes.maybe_global_state_bytes(),
        );
        let global_state_update = match self.protocol_config.get_update_mapping() {
            Ok(global_state_update) => global_state_update,
            Err(err) => {
                return Err(format!("failed to generate global state update: {}", err));
            }
        };

        Ok(UpgradeConfig::new(
            pre_state_hash,
            current_protocol_version,
            self.protocol_config.version,
            Some(era_id),
            Some(self.core_config.validator_slots),
            Some(self.core_config.auction_delay),
            Some(self.core_config.locked_funds_period.millis()),
            Some(self.core_config.round_seigniorage_rate),
            Some(self.core_config.unbonding_delay),
            global_state_update,
            chainspec_registry,
        ))
    }

    /// The maximum number of blocks per era, based on minimum block time, era duration and era
    /// height.
    pub(crate) fn max_blocks_per_era(&self) -> u64 {
        let era_millis = self.core_config.era_duration.millis();
        let round_millis = self.core_config.minimum_block_time.millis();
        // If the last block was above minimum era height, its predecessor's timestamp must have
        // been less than era_millis, if the era start was at 0.
        let latest_timestamp = era_millis.saturating_add(round_millis).saturating_sub(1);
        // Its timestamp determines the maximum number of rounds.
        let max_blocks_by_time = latest_timestamp
            .saturating_div(round_millis)
            .saturating_add(1); // Avoid the fencepost error! First block could be at 0.

        // We produce at least minimum_era_height blocks, even after era_duration has passed.
        max_blocks_by_time.max(self.core_config.minimum_era_height)
    }

    /// Returns `Some` if the validator set is being modified by the upgrade (otherwise `None`)
    /// and `true` if the provided public key is a member of the new set, otherwise `false`.
    #[allow(unused)] // check w/ Fraser...seems odd that we don't care about this anywhere else
    pub(crate) fn is_in_modified_validator_set(&self, public_key: &PublicKey) -> Option<bool> {
        self.protocol_config
            .global_state_update
            .as_ref()
            .and_then(|global_state_update| {
                if global_state_update.validators.is_empty() {
                    return None;
                }
                Some(global_state_update.validators.contains(public_key))
            })
    }
}

#[cfg(test)]
impl Chainspec {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let protocol_config = ProtocolConfig::random(rng);
        let network_config = NetworkConfig::random(rng);
        let core_config = CoreConfig::random(rng);
        let highway_config = HighwayConfig::random(rng);
        let deploy_config = DeployConfig::random(rng);
        let wasm_costs_config = rng.gen();
        let system_costs_config = rng.gen();

        Chainspec {
            protocol_config,
            network_config,
            core_config,
            highway_config,
            deploy_config,
            wasm_config: wasm_costs_config,
            system_costs_config,
        }
    }
}

impl ToBytes for Chainspec {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.protocol_config.to_bytes()?);
        buffer.extend(self.network_config.to_bytes()?);
        buffer.extend(self.core_config.to_bytes()?);
        buffer.extend(self.highway_config.to_bytes()?);
        buffer.extend(self.deploy_config.to_bytes()?);
        buffer.extend(self.wasm_config.to_bytes()?);
        buffer.extend(self.system_costs_config.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.protocol_config.serialized_length()
            + self.network_config.serialized_length()
            + self.core_config.serialized_length()
            + self.highway_config.serialized_length()
            + self.deploy_config.serialized_length()
            + self.wasm_config.serialized_length()
            + self.system_costs_config.serialized_length()
    }
}

impl FromBytes for Chainspec {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_config, remainder) = ProtocolConfig::from_bytes(bytes)?;
        let (network_config, remainder) = NetworkConfig::from_bytes(remainder)?;
        let (core_config, remainder) = CoreConfig::from_bytes(remainder)?;
        let (highway_config, remainder) = HighwayConfig::from_bytes(remainder)?;
        let (deploy_config, remainder) = DeployConfig::from_bytes(remainder)?;
        let (wasm_config, remainder) = WasmConfig::from_bytes(remainder)?;
        let (system_costs_config, remainder) = SystemConfig::from_bytes(remainder)?;
        let chainspec = Chainspec {
            protocol_config,
            network_config,
            core_config,
            highway_config,
            deploy_config,
            wasm_config,
            system_costs_config,
        };
        Ok((chainspec, remainder))
    }
}

impl Loadable for (Chainspec, ChainspecRawBytes) {
    type Error = Error;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        parse_toml::parse_toml(path.as_ref().join(CHAINSPEC_FILENAME))
    }
}

impl From<&Chainspec> for ExecConfig {
    fn from(chainspec: &Chainspec) -> Self {
        ExecConfig::new(
            chainspec.network_config.accounts_config.clone().into(),
            chainspec.wasm_config,
            chainspec.system_costs_config,
            chainspec.core_config.validator_slots,
            chainspec.core_config.auction_delay,
            chainspec.core_config.locked_funds_period.millis(),
            chainspec.core_config.round_seigniorage_rate,
            chainspec.core_config.unbonding_delay,
            chainspec
                .protocol_config
                .activation_point
                .genesis_timestamp()
                .map_or(0, |timestamp| timestamp.millis()),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use num_rational::Ratio;
    use once_cell::sync::Lazy;

    use casper_execution_engine::shared::{
        host_function_costs::{HostFunction, HostFunctionCosts},
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        wasm_config::WasmConfig,
    };
    use casper_types::{EraId, Motes, ProtocolVersion, StoredValue, TimeDiff, Timestamp, U512};

    use super::*;
    use crate::utils::RESOURCES_PATH;

    static EXPECTED_GENESIS_HOST_FUNCTION_COSTS: Lazy<HostFunctionCosts> =
        Lazy::new(|| HostFunctionCosts {
            read_value: HostFunction::new(127, [0, 1, 0]),
            dictionary_get: HostFunction::new(128, [0, 1, 0]),
            write: HostFunction::new(140, [0, 1, 0, 2]),
            dictionary_put: HostFunction::new(141, [0, 1, 2, 3]),
            add: HostFunction::new(100, [0, 1, 2, 3]),
            new_uref: HostFunction::new(122, [0, 1, 2]),
            load_named_keys: HostFunction::new(121, [0, 1]),
            ret: HostFunction::new(133, [0, 1]),
            get_key: HostFunction::new(113, [0, 1, 2, 3, 4]),
            has_key: HostFunction::new(119, [0, 1]),
            put_key: HostFunction::new(125, [0, 1, 2, 3]),
            remove_key: HostFunction::new(132, [0, 1]),
            revert: HostFunction::new(134, [0]),
            is_valid_uref: HostFunction::new(120, [0, 1]),
            add_associated_key: HostFunction::new(101, [0, 1, 2]),
            remove_associated_key: HostFunction::new(129, [0, 1]),
            update_associated_key: HostFunction::new(139, [0, 1, 2]),
            set_action_threshold: HostFunction::new(135, [0, 1]),
            get_caller: HostFunction::new(112, [0]),
            get_blocktime: HostFunction::new(111, [0]),
            create_purse: HostFunction::new(108, [0, 1]),
            transfer_to_account: HostFunction::new(138, [0, 1, 2, 3, 4, 5, 6]),
            transfer_from_purse_to_account: HostFunction::new(136, [0, 1, 2, 3, 4, 5, 6, 7, 8]),
            transfer_from_purse_to_purse: HostFunction::new(137, [0, 1, 2, 3, 4, 5, 6, 7]),
            get_balance: HostFunction::new(110, [0, 1, 2]),
            get_phase: HostFunction::new(117, [0]),
            get_system_contract: HostFunction::new(118, [0, 1, 2]),
            get_main_purse: HostFunction::new(114, [0]),
            read_host_buffer: HostFunction::new(126, [0, 1, 2]),
            create_contract_package_at_hash: HostFunction::new(106, [0, 1]),
            create_contract_user_group: HostFunction::new(107, [0, 1, 2, 3, 4, 5, 6, 7]),
            add_contract_version: HostFunction::new(102, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
            disable_contract_version: HostFunction::new(109, [0, 1, 2, 3]),
            call_contract: HostFunction::new(104, [0, 1, 2, 3, 4, 5, 6]),
            call_versioned_contract: HostFunction::new(105, [0, 1, 2, 3, 4, 5, 6, 7, 8]),
            get_named_arg_size: HostFunction::new(116, [0, 1, 2]),
            get_named_arg: HostFunction::new(115, [0, 1, 2, 3]),
            remove_contract_user_group: HostFunction::new(130, [0, 1, 2, 3]),
            provision_contract_user_group_uref: HostFunction::new(124, [0, 1, 2, 3, 4]),
            remove_contract_user_group_urefs: HostFunction::new(131, [0, 1, 2, 3, 4, 5]),
            print: HostFunction::new(123, [0, 1]),
            blake2b: HostFunction::new(133, [0, 1, 2, 3]),
            random_bytes: HostFunction::new(123, [0, 1]),
        });
    static EXPECTED_GENESIS_WASM_COSTS: Lazy<WasmConfig> = Lazy::new(|| {
        WasmConfig::new(
            17, // initial_memory
            19, // max_stack_height
            EXPECTED_GENESIS_COSTS,
            EXPECTED_GENESIS_STORAGE_COSTS,
            *EXPECTED_GENESIS_HOST_FUNCTION_COSTS,
        )
    });

    const EXPECTED_GENESIS_STORAGE_COSTS: StorageCosts = StorageCosts::new(101);

    const EXPECTED_GENESIS_COSTS: OpcodeCosts = OpcodeCosts {
        bit: 13,
        add: 14,
        mul: 15,
        div: 16,
        load: 17,
        store: 18,
        op_const: 19,
        local: 20,
        global: 21,
        control_flow: 22,
        integer_comparison: 23,
        conversion: 24,
        unreachable: 25,
        nop: 26,
        current_memory: 27,
        grow_memory: 28,
        regular: 29,
    };

    fn check_spec(spec: Chainspec, is_first_version: bool) {
        if is_first_version {
            assert_eq!(
                spec.protocol_config.version,
                ProtocolVersion::from_parts(0, 9, 0)
            );
            assert_eq!(
                spec.protocol_config.activation_point.genesis_timestamp(),
                Some(Timestamp::from(1600454700000))
            );
            assert_eq!(spec.network_config.accounts_config.accounts().len(), 4);

            let accounts: Vec<_> = {
                let mut accounts = spec.network_config.accounts_config.accounts().to_vec();
                accounts.sort_by_key(|account_config| {
                    (account_config.balance(), account_config.bonded_amount())
                });
                accounts
            };

            for (index, account_config) in accounts.into_iter().enumerate() {
                assert_eq!(account_config.balance(), Motes::new(U512::from(index + 1)),);
                assert_eq!(
                    account_config.bonded_amount(),
                    Motes::new(U512::from((index as u64 + 1) * 10))
                );
            }
        } else {
            assert_eq!(
                spec.protocol_config.version,
                ProtocolVersion::from_parts(1, 0, 0)
            );
            assert_eq!(
                spec.protocol_config.activation_point.era_id(),
                EraId::from(1)
            );
            assert!(spec.network_config.accounts_config.accounts().is_empty());
            assert!(spec.protocol_config.global_state_update.is_some());
            assert!(!spec
                .protocol_config
                .global_state_update
                .as_ref()
                .unwrap()
                .validators
                .is_empty());
            for value in spec
                .protocol_config
                .global_state_update
                .unwrap()
                .entries
                .values()
            {
                assert!(StoredValue::from_bytes(value).is_ok());
            }
        }

        assert_eq!(spec.network_config.name, "test-chain");

        assert_eq!(spec.core_config.era_duration, TimeDiff::from(180000));
        assert_eq!(spec.core_config.minimum_era_height, 9);
        assert_eq!(
            spec.core_config.finality_threshold_fraction,
            Ratio::new(2, 25)
        );
        assert_eq!(
            spec.highway_config.maximum_round_length,
            TimeDiff::from(525000)
        );
        assert_eq!(
            spec.highway_config.reduced_reward_multiplier,
            Ratio::new(1, 5)
        );

        assert_eq!(
            spec.deploy_config.max_payment_cost,
            Motes::new(U512::from(9))
        );
        assert_eq!(spec.deploy_config.max_ttl, TimeDiff::from(26300160000));
        assert_eq!(spec.deploy_config.max_dependencies, 11);
        assert_eq!(spec.deploy_config.max_block_size, 12);
        assert_eq!(spec.deploy_config.block_max_deploy_count, 125);
        assert_eq!(spec.deploy_config.block_gas_limit, 13);

        assert_eq!(spec.wasm_config, *EXPECTED_GENESIS_WASM_COSTS);
    }

    #[ignore = "We probably need to reconsider our approach here"]
    #[test]
    fn check_bundled_spec() {
        let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("test/valid/0_9_0");
        check_spec(chainspec, true);
        let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("test/valid/1_0_0");
        check_spec(chainspec, false);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let chainspec = Chainspec::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&chainspec);
    }

    #[test]
    fn should_validate_round_length() {
        let (mut chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        // Minimum block time greater than maximum round length.
        chainspec.core_config.consensus_protocol = ConsensusProtocolName::Highway;
        chainspec.core_config.minimum_block_time = TimeDiff::from(8);
        chainspec.highway_config.maximum_round_length = TimeDiff::from(7);
        assert!(!chainspec.is_valid());

        chainspec.core_config.minimum_block_time = TimeDiff::from(7);
        chainspec.highway_config.maximum_round_length = TimeDiff::from(7);
        assert!(chainspec.is_valid());
    }

    #[ignore = "We probably need to reconsider our approach here"]
    #[test]
    fn should_have_deterministic_chainspec_hash() {
        const PATH: &str = "test/valid/0_9_0";
        const PATH_UNORDERED: &str = "test/valid/0_9_0_unordered";

        let accounts: Vec<u8> = {
            let path = RESOURCES_PATH.join(PATH).join("accounts.toml");
            fs::read(path).expect("should read file")
        };

        let accounts_unordered: Vec<u8> = {
            let path = RESOURCES_PATH.join(PATH_UNORDERED).join("accounts.toml");
            fs::read(path).expect("should read file")
        };

        // Different accounts.toml file content
        assert_ne!(accounts, accounts_unordered);

        let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources(PATH);
        let (chainspec_unordered, _) =
            <(Chainspec, ChainspecRawBytes)>::from_resources(PATH_UNORDERED);

        // Deserializes into equal objects
        assert_eq!(chainspec, chainspec_unordered);

        // With equal hashes
        assert_eq!(chainspec.hash(), chainspec_unordered.hash());
    }

    #[test]
    fn should_compute_max_blocks_per_era() {
        let (mut chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        chainspec.core_config.era_duration = TimeDiff::from(3);
        chainspec.core_config.minimum_era_height = 3;
        // Round length 4.
        chainspec.core_config.minimum_block_time = TimeDiff::from(4);
        // Minimum height is the limiting factor: Three rounds don't fit in 3 ms.
        assert_eq!(3, chainspec.max_blocks_per_era());

        chainspec.core_config.era_duration = TimeDiff::from(12);
        // The block timestamps could be 0, 4, 8, 12. The fourth would be the last one, since it
        // is exactly at the minimum era duration.
        assert_eq!(4, chainspec.max_blocks_per_era());

        chainspec.core_config.era_duration = TimeDiff::from(13);
        // The block timestamps could be 0, 4, 8, 12, 16. The fifth would be the last one, since
        // it is the first to exceed the minimum era duration.
        assert_eq!(5, chainspec.max_blocks_per_era());
    }
}
