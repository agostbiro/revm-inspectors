use alloy_primitives::{Address, Bytes, U256};
use colorchoice::ColorChoice;
use revm::{
    database_interface::EmptyDB,
    specification::hardfork::SpecId,
    wiring::{
        default::{block::BlockEnv, CfgEnv, Env, TransactTo, TxEnv},
        result::{EVMError, ExecutionResult, HaltReason, InvalidTransaction, ResultAndState},
        EthereumWiring,
    },
    Database, DatabaseCommit,
};
use revm_database::CacheDB;
use revm_inspector::{inspector_handle_register, Inspector};
use revm_inspectors::tracing::{
    TraceWriter, TraceWriterConfig, TracingInspector, TracingInspectorConfig,
};
use revm_wiring::{EvmWiring, TransactionValidation};
use std::{convert::Infallible, fmt::Debug};

type TestDb = CacheDB<EmptyDB>;
pub type TestWiring<'a, InspectorT> = EthereumWiring<&'a mut TestDb, InspectorT>;

#[derive(Clone, Debug)]
pub struct TestEvm {
    pub db: TestDb,
    pub env: Box<Env<BlockEnv, TxEnv>>,
    pub spec_id: SpecId,
}

impl Default for TestEvm {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEvm {
    pub fn new() -> Self {
        let db = CacheDB::new(EmptyDB::default());
        let env = Env::boxed(
            CfgEnv::default(),
            BlockEnv { gas_limit: U256::MAX, ..Default::default() },
            TxEnv { gas_limit: u64::MAX, gas_price: U256::ZERO, ..Default::default() },
        );
        Self { db, env, spec_id: SpecId::CANCUN }
    }

    pub fn disable_nonce_check(&mut self) {
        self.env.cfg.disable_nonce_check = true;
    }

    pub fn new_with_spec_id(spec_id: SpecId) -> Self {
        let mut evm = Self::new();
        evm.spec_id = spec_id;
        evm
    }

    pub fn env_with_tx(&self, tx_env: TxEnv) -> Box<Env<BlockEnv, TxEnv>> {
        let mut env = self.env.clone();
        env.tx = tx_env;
        env
    }

    pub fn simple_deploy(&mut self, data: Bytes) -> Address {
        self.deploy(data, TracingInspector::new(TracingInspectorConfig::default_geth()))
            .expect("failed to deploy contract")
    }

    pub fn deploy<InspectorT>(
        &mut self,
        data: Bytes,
        inspector: InspectorT,
    ) -> Result<Address, EVMError<Infallible, InvalidTransaction>>
    where
        InspectorT: for<'a> Inspector<EthereumWiring<&'a mut TestDb, InspectorT>> + Debug,
    {
        let (_, address) = self.try_deploy(data, inspector)?;
        Ok(address.expect("failed to deploy contract"))
    }

    pub fn try_deploy<InspectorT>(
        &mut self,
        data: Bytes,
        inspector: InspectorT,
    ) -> Result<
        (ExecutionResult<HaltReason>, Option<Address>),
        EVMError<Infallible, InvalidTransaction>,
    >
    where
        InspectorT: for<'a> Inspector<EthereumWiring<&'a mut TestDb, InspectorT>> + Debug,
    {
        self.env.tx.data = data;
        self.env.tx.transact_to = TransactTo::Create;

        let (ResultAndState::<HaltReason> { result, state }, env) =
            self.inspect::<InspectorT>(inspector)?;
        self.db.commit(state);
        self.env = env;
        match &result {
            ExecutionResult::Success { output, .. } => {
                let address = output.address().copied();
                Ok((result, address))
            }
            _ => Ok((result, None)),
        }
    }

    pub fn call<InspectorT>(
        &mut self,
        address: Address,
        data: Bytes,
        inspector: InspectorT,
    ) -> Result<ExecutionResult<HaltReason>, EVMError<Infallible, InvalidTransaction>>
    where
        InspectorT: for<'a> Inspector<EthereumWiring<&'a mut TestDb, InspectorT>> + Debug,
    {
        self.env.tx.data = data;
        self.env.tx.transact_to = TransactTo::Call(address);
        let (ResultAndState { result, state }, env) = self.inspect(inspector)?;
        self.db.commit(state);
        self.env = env;
        Ok(result)
    }

    pub fn inspect<InspectorT>(
        &mut self,
        inspector: InspectorT,
    ) -> Result<
        (ResultAndState<HaltReason>, Box<Env<BlockEnv, TxEnv>>),
        EVMError<Infallible, InvalidTransaction>,
    >
    where
        InspectorT: for<'a> Inspector<EthereumWiring<&'a mut TestDb, InspectorT>> + Debug,
    {
        inspect::<EthereumWiring<&mut TestDb, InspectorT>>(
            &mut self.db,
            self.env.clone(),
            self.spec_id,
            inspector,
        )
    }
}

/// Executes the [EnvWithHandlerCfg] against the given [Database] without committing state changes.
pub fn inspect<'a, EvmWiringT>(
    db: EvmWiringT::Database,
    env: Box<Env<<EvmWiringT as EvmWiring>::Block, <EvmWiringT as EvmWiring>::Transaction>>,
    spec_id: EvmWiringT::Hardfork,
    inspector: EvmWiringT::ExternalContext,
) -> Result<
    (ResultAndState<EvmWiringT::HaltReason>, Box<Env<EvmWiringT::Block, EvmWiringT::Transaction>>),
    EVMError<
        <<EvmWiringT as EvmWiring>::Database as Database>::Error,
        <<EvmWiringT as EvmWiring>::Transaction as TransactionValidation>::ValidationError,
    >,
>
where
    EvmWiringT: revm::EvmWiring + 'a,
    <EvmWiringT as EvmWiring>::Transaction: Default,
    <<EvmWiringT as EvmWiring>::Transaction as TransactionValidation>::ValidationError:
        From<InvalidTransaction>,
    <EvmWiringT as EvmWiring>::Block: Default,
    <EvmWiringT as EvmWiring>::ExternalContext: Inspector<EvmWiringT>,
{
    let mut evm = revm::Evm::<'a, EvmWiringT>::builder()
        .with_db(db)
        .with_external_context(inspector)
        .with_env(env)
        .with_spec_id(spec_id)
        .append_handler_register(inspector_handle_register)
        .build();
    let res = evm.transact()?;
    let (_, env, _) = evm.into_db_and_env_with_handler_cfg();
    Ok((res, env))
}

pub fn write_traces(tracer: &TracingInspector) -> String {
    write_traces_with(tracer, TraceWriterConfig::new().color_choice(ColorChoice::Never))
}

pub fn write_traces_with(tracer: &TracingInspector, config: TraceWriterConfig) -> String {
    let mut w = TraceWriter::with_config(Vec::<u8>::new(), config);
    w.write_arena(tracer.traces()).expect("failed to write traces to Vec<u8>");
    String::from_utf8(w.into_writer()).expect("trace writer wrote invalid UTF-8")
}

pub fn print_traces(tracer: &TracingInspector) {
    // Use `println!` so that the output is captured by the test runner.
    println!("{}", write_traces_with(tracer, TraceWriterConfig::new()));
}

/// Deploys a contract with the given code and deployer address.
pub fn deploy_contract(code: Bytes, deployer: Address, spec_id: SpecId) -> (Address, TestEvm) {
    let mut evm = TestEvm::new();

    evm.env.tx.caller = deployer;
    evm.spec_id = spec_id;

    (evm.simple_deploy(code), evm)
}
