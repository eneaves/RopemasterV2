//! Runtime scaffolding for client-side licensing. `LicenseRuntime` is the
//! source of truth for license status, while `LicenseState` only serves as
//! a compatibility shim that caches the active payload for legacy callers.

pub mod device_binding;
pub mod errors;
pub mod fingerprint;
pub mod key_store;
pub mod keyring;
pub mod service;
pub mod state;

pub use device_binding::DeviceBindingStore;
pub use key_store::{FileBackedKeyStore, InstallationKeyStore};
pub use keyring::default_keyring;
pub use service::{LicenseRuntime, LicenseSummaryStatus};
