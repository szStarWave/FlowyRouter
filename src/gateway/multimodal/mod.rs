pub mod data;
pub mod fingerprint;
pub mod store;

pub use store::{MultimodalRouteHint, MultimodalStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MultimodalStrategy {
    #[default]
    None,
    CachedEdge,
    CachedCloud,
    CachedEdgeFallback,
    Probe,
}

impl From<MultimodalRouteHint> for MultimodalStrategy {
    fn from(h: MultimodalRouteHint) -> Self {
        match h {
            MultimodalRouteHint::CachedEdge => MultimodalStrategy::CachedEdge,
            MultimodalRouteHint::CachedCloud => MultimodalStrategy::CachedCloud,
            MultimodalRouteHint::CachedEdgeFallback => MultimodalStrategy::CachedEdgeFallback,
            MultimodalRouteHint::Probe => MultimodalStrategy::Probe,
        }
    }
}
