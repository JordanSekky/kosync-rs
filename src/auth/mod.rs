//! Credentials of the kosync protocol: the headers a client sends, and the
//! check of them against the stored users.

mod headers;
mod user;

pub use headers::{AUTH_KEY, AUTH_USER, AuthHeaders};
pub use user::AuthenticatedUser;
