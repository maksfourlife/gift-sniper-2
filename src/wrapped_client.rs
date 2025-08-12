use std::{ops::Deref, sync::Arc};

use dialoguer::Input;
use grammers_client::{Client, session::Session};
use sqlx::SqlitePool;

use crate::db::{self, get_session, insert_or_replace_session};

#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(transparent)]
    AppDb(#[from] db::Error),
    #[error(transparent)]
    GrammersAuthorization(#[from] grammers_client::client::auth::AuthorizationError),
    #[error(transparent)]
    GrammersInvocation(#[from] grammers_client::InvocationError),
    #[error(transparent)]
    GrammersSignIn(#[from] grammers_client::SignInError),
    #[error(transparent)]
    Dialoguer(#[from] dialoguer::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct WrappedClient {
    phone_number: String,
    pool: Arc<SqlitePool>,
    client: Client,
}

impl WrappedClient {
    pub async fn new(
        pool: Arc<SqlitePool>,
        phone_number: String,
        api_id: i32,
        api_hash: String,
    ) -> Result<Self> {
        let session = get_session(&*pool, &phone_number)
            .await?
            .unwrap_or_else(Session::new);

        let client = Client::connect(grammers_client::Config {
            session,
            api_id,
            api_hash,
            params: Default::default(),
        })
        .await?;

        let this = Self {
            phone_number,
            pool,
            client,
        };

        if !this.client.is_authorized().await? {
            let login_token = this.client.request_login_code(&this.phone_number).await?;

            let login_code: String = Input::new()
                .with_prompt(format!("Please enter login code for {}", this.phone_number))
                .interact()?;

            this.client.sign_in(&login_token, &login_code).await?;

            this.sync_session().await?;
        }

        Ok(this)
    }

    pub fn phone_number(&self) -> &str {
        &self.phone_number
    }

    pub async fn sync_session(&self) -> Result<()> {
        self.client.sync_update_state();
        insert_or_replace_session(&*self.pool, &self.phone_number, self.client.session()).await?;
        Ok(())
    }
}

impl Deref for WrappedClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}
