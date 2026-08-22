pub mod fuzzy;
pub mod regex;
pub mod semantic;
pub mod simd;

pub use fuzzy::FuzzyMatcher;
pub use regex::{Flags, MatchCaps, StructuralRegex};
pub use semantic::SemanticMatcher;
