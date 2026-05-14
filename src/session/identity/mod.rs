mod clock;
mod writer;

pub(crate) use clock::current_timestamp;
pub(crate) use writer::{reviewer_from_git_config, writer_from_git_config};
