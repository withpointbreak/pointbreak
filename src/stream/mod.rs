mod build;
mod geometry;
mod navigation;

pub use build::BuiltReviewNotesStream;
pub use geometry::{LayoutSnapshot, RevealPosition, RowSpan, ViewportSpec};
pub use navigation::{NavigationCommand, NavigationResult, RevealTarget};
