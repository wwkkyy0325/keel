//! Runtime mechanisms for the Keel kernel: turn FSM, event log, tool
//! dispatch, context assembly, policy resolution, minimal permissions and the
//! mock model used by M1.

mod calculator;
pub mod command;
pub mod context;
pub mod dispatch;
pub mod driver;
pub mod event_log;
pub mod fsm;
pub mod governor;
pub mod interaction;
pub mod mock_model;
pub mod permission;
pub mod profile;
pub mod registry;
pub mod summarizer;
pub mod summary;
pub mod tools;
pub mod trash;
pub mod verification;

pub use context::{
    BuiltContext, ContextBuilder, HistoryStage, LatestStage, MemoryStage, SystemStage,
    TaskStateStage,
};
pub use dispatch::ToolDispatcher;
pub use driver::{KernelOptions, TurnDriver, TurnDriverConfig, TurnOutcome};
pub use event_log::{EventLog, EventRecord};
pub use fsm::{transition, TurnFsm};
pub use governor::{ContextGovernor, Pressure, PressureLevel};
pub use interaction::{DeferAllInteractionPort, InteractionBroker};
pub use keel_policy::{RuntimeConfig, TurnLimits};
pub use mock_model::{MockModel, MockStep};
pub use permission::{PermissionEngine, RootScope};
pub use profile::{from_profile_str, load_profile, validate_config};
pub use registry::ToolRegistry;
pub use summarizer::{
    clip_to_tokens, memo_budget, transcript_text, ProviderSummarizer, Summarized, Summarizer,
    MEMO_BUDGET_CEILING, MEMO_BUDGET_FLOOR, SUMMARY_INPUT_MARGIN,
};
pub use tools::{
    CodeSearchTool, DeleteFileTool, EchoTool, ListDirTool, ListTrashTool, MakeDirTool,
    MoveFileTool, ReadFileTool, RestoreFileTool, ShellTool, TodoTool, WriteFileTool,
};
pub use trash::{LocalTrashStore, TrashEntry, TrashStore};
pub use verification::AcceptanceVerifier;
