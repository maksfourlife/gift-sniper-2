use std::sync::Arc;

use futures::{
    StreamExt,
    future::{join_all, try_join_all},
};
use grammers_client::{
    InvocationError,
    grammers_tl_types::{
        self,
        enums::{Document, InputFileLocation, upload::File},
        functions::upload::GetFile,
        types::InputDocumentFileLocation,
    },
};
use sqlx::SqlitePool;
use teloxide::{
    Bot,
    payloads::{SendMessageSetters, SendPhotoSetters},
    prelude::Requester,
    types::{
        ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, ParseMode, Update,
        UpdateKind,
    },
    update_listeners::{AsUpdateStream, polling_default},
};

use crate::{
    core::{BuyGiftsDestination, MaybeResolvedChannel, buy_gifts},
    db::{self, get_chats, insert_chat},
    wrapped_client::WrappedClient,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Db(#[from] db::Error),
    #[error(transparent)]
    TeloxideRequest(#[from] teloxide::RequestError),
    #[error(transparent)]
    GrammersInvocation(#[from] grammers_client::InvocationError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

const GET_FILE_LIMIT_MAX: i32 = 1024 * 1023;

pub async fn run_bot(
    bot: Arc<Bot>,
    pool: Arc<SqlitePool>,
    clients: Vec<Arc<WrappedClient>>,
    admin_usernames: Arc<[String]>,
    buy_limit: Option<u64>,
    buy_dest: Arc<BuyGiftsDestination>,
) -> Result<()> {
    let clients: Arc<[_]> = clients.into();

    let mut polling = polling_default(bot.clone()).await;

    polling
        .as_stream()
        .for_each_concurrent(None, |update| {
            let bot = bot.clone();
            let pool = pool.clone();
            let clients = clients.clone();
            let admin_usernames = admin_usernames.clone();
            let buy_dest = buy_dest.clone();

            async move {
                let update = match update {
                    Ok(t) => t,
                    Err(err) => {
                        tracing::error!(?err, "failed to receive update");
                        return;
                    }
                };

                let update_id = update.id.0;
                if let Err(err) = on_update(
                    bot,
                    pool,
                    clients,
                    admin_usernames,
                    update,
                    buy_limit,
                    buy_dest,
                )
                .await
                {
                    tracing::debug!(update_id, ?err, "failed to process update");
                }
            }
        })
        .await;

    Ok(())
}

async fn on_update(
    bot: Arc<Bot>,
    pool: Arc<SqlitePool>,
    clients: Arc<[Arc<WrappedClient>]>,
    admin_usernames: Arc<[String]>,
    update: Update,
    buy_limit: Option<u64>,
    buy_dest: Arc<BuyGiftsDestination>,
) -> Result<()> {
    tracing::trace!(?update);

    match update.kind {
        UpdateKind::Message(message) => {
            let is_from_admin = match &message.from {
                Some(user) => {
                    user.username.is_some()
                        && admin_usernames.contains(user.username.as_ref().unwrap())
                }
                _ => false,
            };
            if !is_from_admin {
                tracing::debug!(user = ?message.from, "user not in admins list");
                bot.send_message(message.chat.id, "User not in admins list")
                    .await?;

                return Ok(());
            }

            let result = insert_chat(&*pool, message.chat.id.0).await;
            let is_unique_violation = match &result {
                Err(db::Error::Sqlx(sqlx::Error::Database(err))) => err.is_unique_violation(),
                _ => false,
            };
            if !is_unique_violation {
                result?;
            }

            tracing::debug!(chat_id = message.chat.id.0, "added to trusted chats");
            bot.send_message(message.chat.id, "Added to trusted chats")
                .await?;
        }
        UpdateKind::CallbackQuery(callback_query) => {
            let Some(callback_data) = callback_query.data.as_deref() else {
                tracing::debug!(
                    callback_query_id = callback_query.id.0,
                    user_id = callback_query.from.id.0,
                    "callback_query.data is None"
                );
                return Ok(());
            };
            let gift_id: i64 = match callback_data.parse() {
                Ok(t) => t,
                Err(err) => {
                    tracing::error!(
                        callback_query_id = callback_query.id.0,
                        user_id = callback_query.from.id.0,
                        ?err,
                        "failed to parse gift_id"
                    );
                    return Ok(());
                }
            };
            bot.answer_callback_query(callback_query.id).await?;
            tokio::spawn(async move {
                buy_gifts(
                    &clients,
                    bot.clone(),
                    pool.clone(),
                    vec![gift_id],
                    None,
                    buy_limit,
                    &buy_dest,
                )
                .await
                .inspect_err(|err| tracing::error!(?err, "buy_gifts exited with error"))
            });
        }
        _ => tracing::trace!("update skipped"),
    }

    Ok(())
}

pub async fn notify_gifts(
    bot: Arc<Bot>,
    pool: Arc<SqlitePool>,
    client: Arc<WrappedClient>,
    gifts: Vec<grammers_tl_types::types::StarGift>,
) -> Result<()> {
    let chats: Arc<[i64]> = get_chats(&*pool).await?.into();

    join_all(
        gifts
            .iter()
            .filter_map(|gift| match &gift.sticker {
                Document::Document(document) => Some((gift, document)),
                Document::Empty(_) => None,
            })
            .map(|(gift, document)| {
                let request = GetFile {
                    precise: true,
                    cdn_supported: false,
                    location: InputFileLocation::InputDocumentFileLocation(
                        InputDocumentFileLocation {
                            id: document.id,
                            access_hash: document.access_hash,
                            file_reference: document.file_reference.clone(),
                            thumb_size: "s".to_string(),
                        },
                    ),
                    offset: 0,
                    limit: GET_FILE_LIMIT_MAX,
                };

                let client = client.clone();
                let bot = bot.clone();
                let chats = chats.clone();

                async move {
                    // let span = tracing::info_span!("notify_gift", gift_id = gift.id);
                    // let _guard = span.enter();

                    let file = client
                        .invoke_in_dc(&request, document.dc_id)
                        .await
                        .inspect_err(|err| {
                            tracing::error!(?err, gift_id = gift.id, "failed to get file")
                        })?;

                    if let File::File(file) = file {
                        let caption = format!(
                            "ID: `{}`\n\n\
                            Limited: *{}*\n\n\
                            Stars: *{}* ⭐️\n\n\
                            Supply: *{:?}*\n\
                            Remains: *{:?}*",
                            gift.id,
                            gift.limited,
                            gift.stars,
                            gift.availability_total,
                            gift.availability_remains,
                        );

                        let inline_keyboard =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "Buy",
                                gift.id.to_string(),
                            )]]);

                        let input_file = InputFile::memory(file.bytes);

                        try_join_all(chats.iter().map(|chat_id| {
                            let bot = bot.clone();
                            let caption = caption.clone();
                            let inline_keyboard = inline_keyboard.clone();
                            let input_file = input_file.clone();
                            async move {
                                bot.send_photo(ChatId(*chat_id), input_file)
                                    .caption(caption)
                                    .reply_markup(inline_keyboard)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await
                                    .inspect_err(|err| {
                                        tracing::error!(
                                            ?err,
                                            gift_id = gift.id,
                                            "failed to send photo"
                                        )
                                    })
                            }
                        }))
                        .await?;
                    }

                    Result::<_, Error>::Ok(())
                }
            }),
    )
    .await;

    Ok(())
}

#[derive(Debug)]
pub enum GiftBuyStatus {
    PaymentFormError(InvocationError),
    SendStarsFormError(InvocationError),
    Success,
}

pub async fn notify_gift_buy_status(
    bot: &Bot,
    pool: &SqlitePool,
    count: u64,
    phone_number: &str,
    balance: i64,
    gift_id: i64,
    status: GiftBuyStatus,
) -> Result<()> {
    let chats: Arc<[i64]> = get_chats(pool).await?.into();

    let use_markdown_v2 = match status {
        GiftBuyStatus::PaymentFormError(_) | GiftBuyStatus::SendStarsFormError(_) => false,
        GiftBuyStatus::Success => true,
    };

    let title = match status {
        GiftBuyStatus::PaymentFormError(err) => format!("❌ Error\\(PaymentForm\\): {err}"),
        GiftBuyStatus::SendStarsFormError(err) => format!("❌ Error\\(SendStarsForm\\): {err}"),
        GiftBuyStatus::Success => "✅ Gift bought".to_string(),
    };

    try_join_all(chats.iter().map(|chat_id| {
        let text = format!(
            "{title}\n\n\
            Count: *{count}*\n\
            Phone Number: *{}*\n\
            Balance: {balance} ⭐️\n\
            ID: `{gift_id}`",
            phone_number.replace("+", "\\+")
        );
        let mut builder = bot.send_message(ChatId(*chat_id), text);
        if use_markdown_v2 {
            builder = builder.parse_mode(ParseMode::MarkdownV2)
        }
        builder.into_future()
    }))
    .await?;

    Ok(())
}
