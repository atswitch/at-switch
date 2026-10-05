mod auth;
mod binding;
mod client;
mod types;
#[cfg(any(target_os = "windows", test))]
mod windows_auth;

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod live_tests;

pub use auth::*;
pub(crate) use binding::*;
pub use client::*;
pub use types::*;
