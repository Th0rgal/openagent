//! Agent tree structure and management.

use std::collections::HashMap;
use std::sync::Arc;

use super::{Agent, AgentId, AgentType};

/// Reference to an agent in the tree.
pub type AgentRef = Arc<dyn Agent>;

/// The agent tree structure.
/// 
/// # Structure
/// - Root agent at the top
/// - Node agents as intermediate orchestrators
/// - Leaf agents doing specialized work
/// 
/// # Invariants
/// - Exactly one root agent
/// - All non-root agents have a parent
/// - No cycles in parent-child relationships
pub struct AgentTree {
    /// All agents indexed by ID
    agents: HashMap<AgentId, AgentRef>,
    
    /// Parent-child relationships
    children: HashMap<AgentId, Vec<AgentId>>,
    
    /// Root agent ID
    root_id: Option<AgentId>,
}

impl AgentTree {
    /// Create a new empty agent tree.
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            children: HashMap::new(),
            root_id: None,
        }
    }

    /// Set the root agent.
    /// 
    /// # Preconditions
    /// - No root agent currently set
    /// 
    /// # Errors
    /// Returns error if root already exists.
    pub fn set_root(&mut self, agent: AgentRef) -> Result<(), TreeError> {
        if self.root_id.is_some() {
            return Err(TreeError::RootAlreadyExists);
        }

        let id = *agent.id();
        self.agents.insert(id, agent);
        self.children.insert(id, Vec::new());
        self.root_id = Some(id);
        Ok(())
    }

    /// Add a child agent to a parent.
    /// 
    /// # Preconditions
    /// - Parent exists in the tree
    /// - Child is not already in the tree
    pub fn add_child(&mut self, parent_id: AgentId, child: AgentRef) -> Result<(), TreeError> {
        if !self.agents.contains_key(&parent_id) {
            return Err(TreeError::ParentNotFound(parent_id));
        }

        let child_id = *child.id();
        
        if self.agents.contains_key(&child_id) {
            return Err(TreeError::AgentAlreadyExists(child_id));
        }

        self.agents.insert(child_id, child);
        self.children.insert(child_id, Vec::new());
        self.children.get_mut(&parent_id).unwrap().push(child_id);
        
        Ok(())
    }

    /// Get the root agent.
    pub fn root(&self) -> Option<AgentRef> {
        self.root_id.and_then(|id| self.agents.get(&id).cloned())
    }

    /// Get an agent by ID.
    pub fn get(&self, id: &AgentId) -> Option<AgentRef> {
        self.agents.get(id).cloned()
    }

    /// Get children of an agent.
    pub fn get_children(&self, id: &AgentId) -> Vec<AgentRef> {
        self.children
            .get(id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.agents.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find agents by type.
    pub fn find_by_type(&self, agent_type: AgentType) -> Vec<AgentRef> {
        self.agents
            .values()
            .filter(|a| a.agent_type() == agent_type)
            .cloned()
            .collect()
    }

    /// Get all agents in the tree.
    pub fn all_agents(&self) -> Vec<AgentRef> {
        self.agents.values().cloned().collect()
    }

    /// Get the number of agents in the tree.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

impl Default for AgentTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors in tree operations.
#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("Root agent already exists")]
    RootAlreadyExists,
    
    #[error("Parent agent not found: {0}")]
    ParentNotFound(AgentId),
    
    #[error("Agent already exists in tree: {0}")]
    AgentAlreadyExists(AgentId),
}

