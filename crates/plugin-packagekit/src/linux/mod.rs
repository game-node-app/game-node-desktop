//! Linux-only package-manager discovery provider.
//!
//! The provider offers discovery only: it reads installed desktop entries, asks the package
//! manager which installed package owns each entry, and reports the candidates through the SDK's
//! `OperationResult.discovery` event. It never executes a desktop entry command and declares no
//! account, catalog, playtime, launch or install authority.

mod plugin;
mod scan;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod reference_host;

pub(crate) use plugin::run;
