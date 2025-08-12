use grammers_client::session::Session;
use sqlx::SqliteExecutor;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    GrammersSession(#[from] grammers_client::session::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub async fn insert_or_replace_session<'a, E: SqliteExecutor<'a>>(
    executor: E,
    phone_number: &str,
    session: &Session,
) -> Result<()> {
    sqlx::query("INSERT OR REPLACE INTO sessions (phone_number, session) VALUES ($1, $2)")
        .bind(phone_number)
        .bind(session.save())
        .execute(executor)
        .await?;
    Ok(())
}

pub async fn get_session<'a, E: SqliteExecutor<'a>>(
    executor: E,
    phone_number: &str,
) -> Result<Option<Session>> {
    let opt = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT session FROM sessions WHERE phone_number = $1 LIMIT 1",
    )
    .bind(phone_number)
    .fetch_optional(executor)
    .await?;
    Ok(match opt {
        Some(data) => Some(Session::load(&data)?),
        _ => None,
    })
}

pub async fn insert_chat<'a, E: SqliteExecutor<'a>>(executor: E, chat_id: i64) -> Result<()> {
    sqlx::query("INSERT INTO chats(chat_id) VALUES ($1)")
        .bind(chat_id)
        .execute(executor)
        .await?;
    Ok(())
}

pub async fn get_chats<'a, E: SqliteExecutor<'a> + 'a>(executor: E) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar("SELECT chat_id FROM chats")
        .fetch_all(executor)
        .await?)
}
