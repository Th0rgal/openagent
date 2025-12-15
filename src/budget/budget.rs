//! Budget tracking for tasks.
//!
//! # Invariants
//! - `allocated_cents <= total_cents` (enforced at all times)
//! - `spent_cents <= allocated_cents` (enforced at all times)

use serde::{Deserialize, Serialize};

/// Budget for a task, tracking total, allocated, and spent amounts.
/// 
/// # Invariants
/// - `allocated_cents <= total_cents`
/// - `spent_cents <= allocated_cents`
/// 
/// # Design for Provability
/// All mutations go through methods that enforce invariants.
/// Direct field access is prevented (fields are private).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    /// Total budget available (in cents)
    total_cents: u64,
    
    /// Amount allocated to subtasks (in cents)
    allocated_cents: u64,
    
    /// Amount actually spent (in cents)
    spent_cents: u64,
}

impl Budget {
    /// Create a new budget with the given total.
    /// 
    /// # Postconditions
    /// - `budget.total_cents == total_cents`
    /// - `budget.allocated_cents == 0`
    /// - `budget.spent_cents == 0`
    pub fn new(total_cents: u64) -> Self {
        Self {
            total_cents,
            allocated_cents: 0,
            spent_cents: 0,
        }
    }

    /// Create a budget with unlimited funds (for testing).
    /// 
    /// # Warning
    /// This should only be used for testing, not production.
    pub fn unlimited() -> Self {
        Self {
            total_cents: u64::MAX,
            allocated_cents: 0,
            spent_cents: 0,
        }
    }

    // Getters

    /// Get the total budget in cents.
    pub fn total_cents(&self) -> u64 {
        self.total_cents
    }

    /// Get the allocated amount in cents.
    pub fn allocated_cents(&self) -> u64 {
        self.allocated_cents
    }

    /// Get the spent amount in cents.
    pub fn spent_cents(&self) -> u64 {
        self.spent_cents
    }

    /// Get the remaining unallocated budget in cents.
    /// 
    /// # Property
    /// `remaining_cents() == total_cents - allocated_cents`
    pub fn remaining_cents(&self) -> u64 {
        self.total_cents.saturating_sub(self.allocated_cents)
    }

    /// Get the unspent allocated budget in cents.
    /// 
    /// # Property
    /// `unspent_cents() == allocated_cents - spent_cents`
    pub fn unspent_cents(&self) -> u64 {
        self.allocated_cents.saturating_sub(self.spent_cents)
    }

    /// Check if there's any remaining budget to allocate.
    pub fn has_remaining(&self) -> bool {
        self.remaining_cents() > 0
    }

    /// Check if the budget is exhausted (all spent).
    pub fn is_exhausted(&self) -> bool {
        self.spent_cents >= self.allocated_cents
    }

    // Mutations with invariant enforcement

    /// Allocate some budget for a subtask.
    /// 
    /// # Precondition
    /// `amount <= self.remaining_cents()`
    /// 
    /// # Postcondition
    /// `self.allocated_cents` increases by `amount`
    /// 
    /// # Errors
    /// Returns `Err` if allocation would exceed total.
    pub fn allocate(&mut self, amount: u64) -> Result<(), BudgetError> {
        let new_allocated = self.allocated_cents.saturating_add(amount);
        
        if new_allocated > self.total_cents {
            return Err(BudgetError::AllocationExceedsTotal {
                requested: amount,
                remaining: self.remaining_cents(),
            });
        }
        
        self.allocated_cents = new_allocated;
        Ok(())
    }

    /// Record spending against the allocated budget.
    /// 
    /// # Precondition
    /// `amount <= self.unspent_cents()`
    /// 
    /// # Postcondition
    /// `self.spent_cents` increases by `amount`
    /// 
    /// # Errors
    /// Returns `Err` if spending would exceed allocated.
    pub fn spend(&mut self, amount: u64) -> Result<(), BudgetError> {
        let new_spent = self.spent_cents.saturating_add(amount);
        
        if new_spent > self.allocated_cents {
            return Err(BudgetError::SpendingExceedsAllocated {
                requested: amount,
                available: self.unspent_cents(),
            });
        }
        
        self.spent_cents = new_spent;
        Ok(())
    }

    /// Try to spend, returning how much was actually spent.
    /// 
    /// This is a "best effort" version that won't fail,
    /// but may spend less than requested.
    /// 
    /// # Postcondition
    /// `result <= amount`
    /// `result <= self.unspent_cents()` (before call)
    pub fn try_spend(&mut self, amount: u64) -> u64 {
        let available = self.unspent_cents();
        let actual = amount.min(available);
        self.spent_cents += actual;
        actual
    }

    /// Check if we can afford a given cost.
    /// 
    /// # Returns
    /// `true` if `cost <= self.unspent_cents()`
    pub fn can_afford(&self, cost: u64) -> bool {
        cost <= self.unspent_cents()
    }

    /// Create a sub-budget from this budget.
    /// 
    /// # Precondition
    /// `amount <= self.remaining_cents()`
    /// 
    /// # Side Effects
    /// Allocates `amount` from this budget.
    /// 
    /// # Returns
    /// A new budget with `total_cents == amount`.
    pub fn create_sub_budget(&mut self, amount: u64) -> Result<Budget, BudgetError> {
        self.allocate(amount)?;
        Ok(Budget::new(amount))
    }
}

impl Default for Budget {
    /// Default budget is $1.00 (100 cents).
    fn default() -> Self {
        Self::new(100)
    }
}

/// Errors related to budget operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BudgetError {
    #[error("Allocation of {requested} cents exceeds remaining budget of {remaining} cents")]
    AllocationExceedsTotal { requested: u64, remaining: u64 },
    
    #[error("Spending of {requested} cents exceeds available budget of {available} cents")]
    SpendingExceedsAllocated { requested: u64, available: u64 },
    
    #[error("Insufficient budget: need {needed} cents, have {available} cents")]
    InsufficientBudget { needed: u64, available: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_invariants() {
        let mut budget = Budget::new(100);
        
        // Initially, nothing is allocated or spent
        assert_eq!(budget.remaining_cents(), 100);
        assert_eq!(budget.unspent_cents(), 0);
        
        // Allocate some
        budget.allocate(50).unwrap();
        assert_eq!(budget.remaining_cents(), 50);
        assert_eq!(budget.unspent_cents(), 50);
        
        // Spend some
        budget.spend(30).unwrap();
        assert_eq!(budget.unspent_cents(), 20);
        assert_eq!(budget.spent_cents(), 30);
        
        // Can't over-allocate
        assert!(budget.allocate(60).is_err());
        
        // Can't over-spend
        assert!(budget.spend(30).is_err());
    }

    #[test]
    fn test_sub_budget() {
        let mut parent = Budget::new(100);
        
        let child = parent.create_sub_budget(40).unwrap();
        
        assert_eq!(parent.remaining_cents(), 60);
        assert_eq!(child.total_cents(), 40);
        assert_eq!(child.remaining_cents(), 40);
    }
}

