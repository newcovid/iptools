//! Transitional imports for the legacy native modules.
//!
//! All definitions live in `iptools-core`; this module can disappear when the
//! old native state root is removed.

pub use iptools_core::{
    AdapterEditParams, AdapterEditPersist, HistoryPersist, LanSpeedPersist, LinkParams,
    LinkQualityPersist, PingPersist, PortScanPersist, ScannerPersist, SessionState, TracePersist,
    UiPersist,
};
