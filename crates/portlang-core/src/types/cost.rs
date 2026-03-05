use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign};

/// Cost represented in microdollars ($1.00 = 1,000,000 microdollars)
/// This avoids floating-point precision issues in cost tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cost(u64);

impl Cost {
    /// Create a Cost from dollars (will be converted to microdollars)
    pub fn from_dollars(dollars: f64) -> Self {
        Cost((dollars * 1_000_000.0) as u64)
    }

    /// Create a Cost directly from microdollars
    pub fn from_microdollars(microdollars: u64) -> Self {
        Cost(microdollars)
    }

    /// Convert cost to dollars for display
    pub fn to_dollars(&self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    /// Get raw microdollars value
    pub fn microdollars(&self) -> u64 {
        self.0
    }

    /// Zero cost
    pub const ZERO: Cost = Cost(0);
}

impl Add for Cost {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Cost(self.0 + other.0)
    }
}

impl AddAssign for Cost {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl fmt::Display for Cost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${:.6}", self.to_dollars())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_conversion() {
        let cost = Cost::from_dollars(1.50);
        assert_eq!(cost.microdollars(), 1_500_000);
        assert_eq!(cost.to_dollars(), 1.5);
    }

    #[test]
    fn test_cost_arithmetic() {
        let a = Cost::from_dollars(1.0);
        let b = Cost::from_dollars(0.5);
        assert_eq!((a + b).to_dollars(), 1.5);
    }

    #[test]
    fn test_cost_ordering() {
        let a = Cost::from_dollars(1.0);
        let b = Cost::from_dollars(2.0);
        assert!(a < b);
    }
}
