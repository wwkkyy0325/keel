//! Runtime mechanisms for the Keel kernel: turn FSM, event log, tool
//! dispatch, context assembly, policy resolution, minimal permissions and the
//! mock model used by M1.

mod calculator;
pub mod context;
pub mod dispatch;
pub mod driver;
pub mod event_log;
pub mod fsm;
pub mod interaction;
pub mod mock_model;
pub mod permission;
pub mod policy;
pub mod profile;
pub mod registry;
pub mod tools;
pub mod trash;
pub mod verification;

pub use context::{BuiltContext, ContextBuilder, LatestStage, SystemStage};
pub use dispatch::ToolDispatcher;
pub use driver::{TurnDriver, TurnDriverConfig, TurnOutcome};
pub use event_log::{EventLog, EventRecord};
pub use fsm::{transition, TurnFsm};
pub use interaction::{DeferAllInteractionPort, InteractionBroker};
pub use mock_model::{MockModel, MockStep};
pub use permission::PermissionEngine;
pub use policy::{RuntimeConfig, TurnLimits};
pub use profile::{from_profile_str, load_profile, validate_config};
pub use registry::ToolRegistry;
pub use tools::{EchoTool, ReadFileTool, WriteFileTool};
pub use trash::{LocalTrashStore, TrashEntry, TrashStore};
pub use verification::AcceptanceVerifier;
