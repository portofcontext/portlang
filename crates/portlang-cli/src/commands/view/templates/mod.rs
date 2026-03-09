pub mod diff;
pub mod eval;
pub mod field;
pub mod trajectory;

// Re-export commonly used functions
pub use trajectory::generate_trajectory_html_with_back_link;
