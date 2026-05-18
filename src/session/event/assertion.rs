use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionMode {
    #[default]
    Advisory,
    Operative,
}

pub(super) fn is_default_advisory(mode: &AssertionMode) -> bool {
    *mode == AssertionMode::Advisory
}
