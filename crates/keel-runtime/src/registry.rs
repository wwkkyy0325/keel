use std::collections::BTreeMap;
use std::sync::Arc;

use keel_core::{KernelError, Tool, ToolSchema};

/// Registered tools, keyed by stable name. `BTreeMap` keeps schema order
/// deterministic.
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), KernelError> {
        let schema = tool.schema();
        if schema.name.is_empty() {
            return Err(KernelError::InvalidConfig {
                message: "tool name must not be empty".to_owned(),
            });
        }
        if self.tools.contains_key(&schema.name) {
            return Err(KernelError::InvalidConfig {
                message: format!("duplicate tool `{}`", schema.name),
            });
        }
        self.tools.insert(schema.name, tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|tool| tool.schema()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
