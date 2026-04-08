use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::snapshot::CompiledSnapshot;

pub fn initial_snapshot_handle(snapshot: CompiledSnapshot) -> Arc<ArcSwap<CompiledSnapshot>> {
    Arc::new(ArcSwap::from_pointee(snapshot))
}
