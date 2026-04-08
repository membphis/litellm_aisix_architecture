use bytes::Bytes;

use crate::usage::Usage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Delta(Bytes),
    Usage(Usage),
    Done,
}
