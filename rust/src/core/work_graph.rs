//! Bounded Work Graph for multi-agent orchestration (P11 / DIM 4).
//!
//! Manages parent/child agent delegation with:
//! - Budget inheritance (child cannot exceed parent)
//! - Fan-out limits (max concurrent children)
//! - Stop conditions (stale, over-budget, redundant)
//! - Provenance tracking for attribution

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const WORK_GRAPH_SCHEMA_VERSION: u16 = 1;
const MAX_GRAPH_NODES: usize = 256;
const MAX_FAN_OUT: usize = 16;
const MAX_DEPTH: u16 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Active,
    Completed,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    BudgetExhausted,
    Stale,
    Redundant,
    ParentStopped,
    ManualStop,
    DepthExceeded,
    FanOutExceeded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkNodeBudget {
    pub tokens_allocated: u64,
    pub tokens_consumed: u64,
    pub cost_micros_allocated: u64,
    pub cost_micros_consumed: u64,
}

impl WorkNodeBudget {
    pub fn tokens_remaining(&self) -> u64 {
        self.tokens_allocated.saturating_sub(self.tokens_consumed)
    }

    pub fn cost_remaining(&self) -> u64 {
        self.cost_micros_allocated
            .saturating_sub(self.cost_micros_consumed)
    }

    pub fn is_exhausted(&self) -> bool {
        self.tokens_remaining() == 0 || self.cost_remaining() == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkNode {
    pub node_id: String,
    pub agent_id: String,
    pub parent_node_id: Option<String>,
    pub capsule_ref: String,
    pub status: NodeStatus,
    pub budget: WorkNodeBudget,
    pub depth: u16,
    pub stop_reason: Option<StopReason>,
    pub outcome_ref: Option<String>,
}

/// Bounded, acyclic work graph with enforced fan-out and budget constraints.
pub struct BoundedWorkGraph {
    nodes: BTreeMap<String, WorkNode>,
    children: BTreeMap<String, Vec<String>>,
    max_fan_out: usize,
    max_depth: u16,
}

impl Default for BoundedWorkGraph {
    fn default() -> Self {
        Self::new(MAX_FAN_OUT, MAX_DEPTH)
    }
}

impl BoundedWorkGraph {
    #[must_use]
    pub fn new(max_fan_out: usize, max_depth: u16) -> Self {
        Self {
            nodes: BTreeMap::new(),
            children: BTreeMap::new(),
            max_fan_out: max_fan_out.min(MAX_FAN_OUT).max(1),
            max_depth: max_depth.min(MAX_DEPTH).max(1),
        }
    }

    /// Add a root node (no parent).
    pub fn add_root(
        &mut self,
        node_id: String,
        agent_id: String,
        capsule_ref: String,
        budget: WorkNodeBudget,
    ) -> Result<&WorkNode, WorkGraphError> {
        if self.nodes.len() >= MAX_GRAPH_NODES {
            return Err(WorkGraphError::CapacityExceeded);
        }
        if self.nodes.contains_key(&node_id) {
            return Err(WorkGraphError::DuplicateNode(node_id));
        }
        let node = WorkNode {
            node_id: node_id.clone(),
            agent_id,
            parent_node_id: None,
            capsule_ref,
            status: NodeStatus::Active,
            budget,
            depth: 0,
            stop_reason: None,
            outcome_ref: None,
        };
        self.nodes.insert(node_id.clone(), node);
        Ok(self.nodes.get(&node_id).unwrap())
    }

    /// Delegate work to a child node. Validates budget inheritance and fan-out.
    pub fn delegate(
        &mut self,
        parent_node_id: &str,
        child_node_id: String,
        child_agent_id: String,
        capsule_ref: String,
        child_budget: WorkNodeBudget,
    ) -> Result<&WorkNode, WorkGraphError> {
        if self.nodes.len() >= MAX_GRAPH_NODES {
            return Err(WorkGraphError::CapacityExceeded);
        }
        if self.nodes.contains_key(&child_node_id) {
            return Err(WorkGraphError::DuplicateNode(child_node_id));
        }
        let parent = self
            .nodes
            .get(parent_node_id)
            .ok_or_else(|| WorkGraphError::NodeNotFound(parent_node_id.to_string()))?;
        if parent.status != NodeStatus::Active {
            return Err(WorkGraphError::ParentNotActive(parent_node_id.to_string()));
        }
        let new_depth = parent.depth + 1;
        if new_depth > self.max_depth {
            return Err(WorkGraphError::DepthExceeded(self.max_depth));
        }
        if child_budget.tokens_allocated > parent.budget.tokens_remaining() {
            return Err(WorkGraphError::BudgetExceedsParent {
                child_requested: child_budget.tokens_allocated,
                parent_remaining: parent.budget.tokens_remaining(),
            });
        }
        if child_budget.cost_micros_allocated > parent.budget.cost_remaining() {
            return Err(WorkGraphError::BudgetExceedsParent {
                child_requested: child_budget.cost_micros_allocated,
                parent_remaining: parent.budget.cost_remaining(),
            });
        }
        let current_children = self.children.get(parent_node_id).map_or(0, Vec::len);
        if current_children >= self.max_fan_out {
            return Err(WorkGraphError::FanOutExceeded(self.max_fan_out));
        }
        let node = WorkNode {
            node_id: child_node_id.clone(),
            agent_id: child_agent_id,
            parent_node_id: Some(parent_node_id.to_string()),
            capsule_ref,
            status: NodeStatus::Active,
            budget: child_budget,
            depth: new_depth,
            stop_reason: None,
            outcome_ref: None,
        };
        self.nodes.insert(child_node_id.clone(), node);
        self.children
            .entry(parent_node_id.to_string())
            .or_default()
            .push(child_node_id.clone());
        Ok(self.nodes.get(&child_node_id).unwrap())
    }

    /// Mark a node as completed with an outcome reference.
    pub fn complete(&mut self, node_id: &str, outcome_ref: String) -> Result<(), WorkGraphError> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| WorkGraphError::NodeNotFound(node_id.to_string()))?;
        if node.status != NodeStatus::Active {
            return Err(WorkGraphError::InvalidTransition(node_id.to_string()));
        }
        node.status = NodeStatus::Completed;
        node.outcome_ref = Some(outcome_ref);
        Ok(())
    }

    /// Stop a node and all its descendants (cascade).
    pub fn stop(
        &mut self,
        node_id: &str,
        reason: StopReason,
    ) -> Result<Vec<String>, WorkGraphError> {
        if !self.nodes.contains_key(node_id) {
            return Err(WorkGraphError::NodeNotFound(node_id.to_string()));
        }
        let mut stopped = Vec::new();
        self.stop_recursive(node_id, reason, &mut stopped);
        Ok(stopped)
    }

    /// Record token consumption on a node.
    pub fn consume_budget(
        &mut self,
        node_id: &str,
        tokens: u64,
        cost_micros: u64,
    ) -> Result<bool, WorkGraphError> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| WorkGraphError::NodeNotFound(node_id.to_string()))?;
        if node.status != NodeStatus::Active {
            return Err(WorkGraphError::InvalidTransition(node_id.to_string()));
        }
        node.budget.tokens_consumed = node.budget.tokens_consumed.saturating_add(tokens);
        node.budget.cost_micros_consumed =
            node.budget.cost_micros_consumed.saturating_add(cost_micros);
        if node.budget.is_exhausted() {
            node.status = NodeStatus::Stopped;
            node.stop_reason = Some(StopReason::BudgetExhausted);
            return Ok(true);
        }
        Ok(false)
    }

    /// Check all nodes for stop conditions and cascade.
    pub fn enforce_stop_conditions(&mut self) -> Vec<(String, StopReason)> {
        let exhausted: Vec<String> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.status == NodeStatus::Active && n.budget.is_exhausted())
            .map(|(id, _)| id.clone())
            .collect();
        let mut stopped = Vec::new();
        for node_id in exhausted {
            let mut cascade = Vec::new();
            self.stop_recursive(&node_id, StopReason::BudgetExhausted, &mut cascade);
            for id in cascade {
                stopped.push((id, StopReason::BudgetExhausted));
            }
        }
        stopped
    }

    pub fn get_node(&self, node_id: &str) -> Option<&WorkNode> {
        self.nodes.get(node_id)
    }

    pub fn children_of(&self, node_id: &str) -> &[String] {
        self.children.get(node_id).map_or(&[], Vec::as_slice)
    }

    pub fn active_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.status == NodeStatus::Active)
            .count()
    }

    pub fn total_count(&self) -> usize {
        self.nodes.len()
    }

    fn stop_recursive(&mut self, node_id: &str, reason: StopReason, stopped: &mut Vec<String>) {
        if let Some(node) = self.nodes.get_mut(node_id) {
            if node.status == NodeStatus::Active || node.status == NodeStatus::Pending {
                node.status = NodeStatus::Stopped;
                node.stop_reason = Some(reason);
                stopped.push(node_id.to_string());
            }
        }
        let children: Vec<String> = self.children.get(node_id).cloned().unwrap_or_default();
        for child_id in children {
            self.stop_recursive(&child_id, StopReason::ParentStopped, stopped);
        }
    }
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum WorkGraphError {
    #[error("graph at capacity ({MAX_GRAPH_NODES} nodes)")]
    CapacityExceeded,
    #[error("duplicate node: {0}")]
    DuplicateNode(String),
    #[error("node not found: {0}")]
    NodeNotFound(String),
    #[error("parent not active: {0}")]
    ParentNotActive(String),
    #[error("depth exceeds max {0}")]
    DepthExceeded(u16),
    #[error("fan-out exceeds max {0}")]
    FanOutExceeded(usize),
    #[error("child budget ({child_requested}) exceeds parent remaining ({parent_remaining})")]
    BudgetExceedsParent {
        child_requested: u64,
        parent_remaining: u64,
    },
    #[error("invalid status transition for node: {0}")]
    InvalidTransition(String),
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(tokens: u64, cost: u64) -> WorkNodeBudget {
        WorkNodeBudget {
            tokens_allocated: tokens,
            tokens_consumed: 0,
            cost_micros_allocated: cost,
            cost_micros_consumed: 0,
        }
    }

    #[test]
    fn basic_delegation_and_budget_inheritance() {
        let mut g = BoundedWorkGraph::default();
        g.add_root(
            "root".into(),
            "parent-agent".into(),
            "capsule:abc".into(),
            budget(1000, 500),
        )
        .unwrap();
        g.delegate(
            "root",
            "child-1".into(),
            "child-agent".into(),
            "capsule:def".into(),
            budget(400, 200),
        )
        .unwrap();
        assert_eq!(g.active_count(), 2);
        assert_eq!(g.children_of("root"), &["child-1"]);
    }

    #[test]
    fn child_cannot_exceed_parent_budget() {
        let mut g = BoundedWorkGraph::default();
        g.add_root(
            "root".into(),
            "a".into(),
            "capsule:x".into(),
            budget(100, 50),
        )
        .unwrap();
        assert!(matches!(
            g.delegate(
                "root",
                "c".into(),
                "b".into(),
                "capsule:y".into(),
                budget(200, 30)
            ),
            Err(WorkGraphError::BudgetExceedsParent { .. })
        ));
    }

    #[test]
    fn fan_out_limit_enforced() {
        let mut g = BoundedWorkGraph::new(2, MAX_DEPTH);
        g.add_root(
            "root".into(),
            "a".into(),
            "capsule:x".into(),
            budget(1000, 1000),
        )
        .unwrap();
        g.delegate(
            "root",
            "c1".into(),
            "b".into(),
            "capsule:1".into(),
            budget(100, 100),
        )
        .unwrap();
        g.delegate(
            "root",
            "c2".into(),
            "b".into(),
            "capsule:2".into(),
            budget(100, 100),
        )
        .unwrap();
        assert!(matches!(
            g.delegate(
                "root",
                "c3".into(),
                "b".into(),
                "capsule:3".into(),
                budget(100, 100)
            ),
            Err(WorkGraphError::FanOutExceeded(2))
        ));
    }

    #[test]
    fn depth_limit_enforced() {
        let mut g = BoundedWorkGraph::new(MAX_FAN_OUT, 2);
        g.add_root(
            "n0".into(),
            "a".into(),
            "capsule:0".into(),
            budget(1000, 1000),
        )
        .unwrap();
        g.delegate(
            "n0",
            "n1".into(),
            "b".into(),
            "capsule:1".into(),
            budget(500, 500),
        )
        .unwrap();
        g.delegate(
            "n1",
            "n2".into(),
            "c".into(),
            "capsule:2".into(),
            budget(200, 200),
        )
        .unwrap();
        assert!(matches!(
            g.delegate(
                "n2",
                "n3".into(),
                "d".into(),
                "capsule:3".into(),
                budget(100, 100)
            ),
            Err(WorkGraphError::DepthExceeded(2))
        ));
    }

    #[test]
    fn stop_cascades_to_children() {
        let mut g = BoundedWorkGraph::default();
        g.add_root(
            "root".into(),
            "a".into(),
            "capsule:r".into(),
            budget(1000, 1000),
        )
        .unwrap();
        g.delegate(
            "root",
            "c1".into(),
            "b".into(),
            "capsule:1".into(),
            budget(300, 300),
        )
        .unwrap();
        g.delegate(
            "c1",
            "gc1".into(),
            "c".into(),
            "capsule:gc".into(),
            budget(100, 100),
        )
        .unwrap();
        let stopped = g.stop("c1", StopReason::Stale).unwrap();
        assert_eq!(stopped, vec!["c1", "gc1"]);
        assert_eq!(g.get_node("c1").unwrap().status, NodeStatus::Stopped);
        assert_eq!(
            g.get_node("gc1").unwrap().stop_reason,
            Some(StopReason::ParentStopped)
        );
    }

    #[test]
    fn budget_exhaustion_auto_stops() {
        let mut g = BoundedWorkGraph::default();
        g.add_root(
            "root".into(),
            "a".into(),
            "capsule:r".into(),
            budget(100, 100),
        )
        .unwrap();
        let exhausted = g.consume_budget("root", 100, 50).unwrap();
        assert!(exhausted);
        assert_eq!(g.get_node("root").unwrap().status, NodeStatus::Stopped);
    }

    #[test]
    fn complete_sets_outcome() {
        let mut g = BoundedWorkGraph::default();
        g.add_root(
            "root".into(),
            "a".into(),
            "capsule:r".into(),
            budget(1000, 1000),
        )
        .unwrap();
        g.complete("root", "outcome:success".into()).unwrap();
        let node = g.get_node("root").unwrap();
        assert_eq!(node.status, NodeStatus::Completed);
        assert_eq!(node.outcome_ref.as_deref(), Some("outcome:success"));
    }
}
