use crate::error::{FieldParseError, Result};
use glob::Pattern;

/// Validate a glob pattern
pub fn validate_glob_pattern(pattern: &str) -> Result<()> {
    Pattern::new(pattern).map_err(|e| FieldParseError::InvalidGlob {
        pattern: pattern.to_string(),
        error: e.to_string(),
    })?;
    Ok(())
}

/// Validate all glob patterns in a list
pub fn validate_glob_patterns(patterns: &[String]) -> Result<()> {
    for pattern in patterns {
        validate_glob_pattern(pattern)?;
    }
    Ok(())
}

/// Parse cost from string or number
pub fn parse_cost(value: &crate::raw::StringOrNumber) -> Result<portlang_core::Cost> {
    use crate::raw::StringOrNumber;

    match value {
        StringOrNumber::String(s) => {
            // Parse "$2.00" format
            let s = s.trim();
            if !s.starts_with('$') {
                return Err(FieldParseError::InvalidCost(format!(
                    "Cost string must start with '$', got: {}",
                    s
                )));
            }

            let number_part = &s[1..];
            let dollars: f64 = number_part.parse().map_err(|_| {
                FieldParseError::InvalidCost(format!("Invalid number in cost: {}", s))
            })?;

            Ok(portlang_core::Cost::from_dollars(dollars))
        }
        StringOrNumber::Number(n) => Ok(portlang_core::Cost::from_dollars(*n)),
    }
}
