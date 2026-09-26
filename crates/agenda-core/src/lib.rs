//! Cœur de l'agenda. Toute la logique passe par [`api::Api::call`].
pub mod api;
pub mod date;
pub mod ical;
pub mod json;
pub mod model;
pub mod nlp;
pub mod rrule;
pub mod search;
pub mod store;
pub mod tz;
pub mod watch;
pub mod yaml;
pub use api::Api;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
