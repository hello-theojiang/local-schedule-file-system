//! Cœur de l'agenda. Toute la logique passe par [`api::Api::call`].
pub mod api;
pub use api::Api;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
