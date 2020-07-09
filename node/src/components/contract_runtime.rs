//! Contract Runtime component.
mod config;
pub mod core;
pub mod shared;
pub mod storage;

use std::{
    fmt::{Debug, Display},
    path::PathBuf,
    sync::Arc,
};

use lmdb::DatabaseFlags;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::components::contract_runtime::core::engine_state::{EngineConfig, EngineState};
use crate::components::contract_runtime::storage::protocol_data_store::lmdb::LmdbProtocolDataStore;
use crate::components::contract_runtime::storage::{
    global_state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment,
    trie_store::lmdb::LmdbTrieStore,
};

use crate::components::Component;
use crate::effect::{Effect, EffectBuilder, Multiple};
use crate::StorageConfig;
pub use config::Config;

/// The contract runtime components.
pub(crate) struct ContractRuntime {
    #[allow(dead_code)]
    engine_state: EngineState<LmdbGlobalState>,
}

impl Debug for ContractRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContractRuntime").finish()
    }
}

/// Contract runtime message used by the pinger.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Message;

/// Pinger component event.
#[derive(Debug)]
pub enum Event {
    /// Foo
    Foo,
    /// Bar
    Bar,
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Foo => write!(f, "foo"),
            Event::Bar => write!(f, "bar"),
        }
    }
}

impl<REv> Component<REv> for ContractRuntime
where
    REv: From<Event> + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Foo => todo!("foo"),
            Event::Bar => todo!("bar"),
        }
    }
}

/// Builds and returns engine global state
fn get_engine_state(
    data_dir: PathBuf,
    map_size: usize,
    engine_config: EngineConfig,
) -> EngineState<LmdbGlobalState> {
    let environment = {
        let ret = LmdbEnvironment::new(&data_dir, map_size).expect("should have lmdb environment");
        Arc::new(ret)
    };

    let trie_store = {
        let ret = LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())
            .expect("should have trie store");
        Arc::new(ret)
    };

    let protocol_data_store = {
        let ret = LmdbProtocolDataStore::new(&environment, None, DatabaseFlags::empty())
            .expect("should have protocol data store");
        Arc::new(ret)
    };

    let global_state = LmdbGlobalState::empty(environment, trie_store, protocol_data_store)
        .expect("should have global state");

    EngineState::new(global_state, engine_config)
}

impl ContractRuntime {
    /// Create and initialize a new pinger.
    pub(crate) fn new<REv: From<Event> + Send>(
        storage_config: &StorageConfig,
        contract_runtime_config: Config,
        _effect_builder: EffectBuilder<REv>,
    ) -> (Self, Multiple<Effect<Event>>) {
        let engine_config = EngineConfig::new()
            .with_use_system_contracts(contract_runtime_config.use_system_contracts)
            .with_enable_bonding(contract_runtime_config.enable_bonding);

        let engine_state = get_engine_state(
            storage_config.path.clone(),
            contract_runtime_config.map_size,
            engine_config,
        );

        let contract_runtime = ContractRuntime { engine_state };

        let init = Multiple::new();

        (contract_runtime, init)
    }
}