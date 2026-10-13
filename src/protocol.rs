//! The JSON bodies of the kosync protocol, as KOReader sends and expects them.
//!
//! Shared by the server's handlers and by [`crate::upstream`], which replays a
//! push against another implementation of the same protocol.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateUserResponse {
    pub username: String,
}

#[derive(Serialize, Debug)]
pub struct AuthResponse {
    /// Always "OK"
    pub authorized: String,
}

impl Default for AuthResponse {
    fn default() -> Self {
        Self {
            authorized: "OK".to_owned(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateProgressRequest {
    pub document: String,
    /// Opaque position: an XPointer for reflowable documents, a page number
    /// for paged ones
    pub progress: String,
    /// In [0, 1]
    pub percentage: f64,
    pub device: String,
    /// The client always sends it, but the reference server doesn't require it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Only sent when the user opts in
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateProgressResponse {
    pub document: String,
    /// Unix seconds
    pub timestamp: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetProgressResponse {
    pub username: String,
    pub document: String,
    pub progress: String,
    pub percentage: f64,
    pub device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Unix seconds
    pub timestamp: i64,
}
