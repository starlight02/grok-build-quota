use leptos::prelude::*;

#[cfg(feature = "ssr")]
mod ssr;
mod types;

pub use types::*;

#[server(CheckAuthFile, "/api")]
pub async fn check_auth_file(
    file: AuthUpload,
    refresh: bool,
) -> Result<CheckResult, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        return Ok(ssr::check_one(ssr::shared_client(), file, refresh).await);
    }

    #[cfg(not(feature = "ssr"))]
    {
        let _ = (file, refresh);
        Err(ServerFnError::new("server only"))
    }
}
