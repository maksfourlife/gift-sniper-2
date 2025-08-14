use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use futures::{TryFutureExt, future::try_join_all};
use grammers_client::{
    grammers_tl_types::{
        enums::{
            InputInvoice, InputPeer, StarGift, StarsAmount,
            payments::{StarGifts, StarsStatus},
        },
        functions::payments::{GetPaymentForm, GetStarGifts, GetStarsStatus, SendStarsForm},
        types::{InputInvoiceStarGift, InputPeerChannel},
    },
    types::Chat,
};
use sqlx::SqlitePool;
use teloxide::Bot;

use crate::{
    bot::{self, GiftBuyStatus, notify_gift_buy_status},
    wrapped_client::WrappedClient,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Bot(#[from] bot::Error),
    #[error(transparent)]
    GrammersInvocation(#[from] grammers_client::InvocationError),
    #[error("gift price not found (gift_id = {0})")]
    GiftPriceNotFound(i64),
    #[error("unexpected not modified")]
    UnexpectedNotModified,
    #[error("chat not found (username = {0})")]
    ChatNotFound(String),
    #[error("chat is not a channel")]
    ChatIsNotChannel,
    #[error("channel not accesible (channel_id = {0})")]
    ChannelNotAccessible(i64),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone)]
pub enum BuyGiftsDestination {
    PeerSelf,
    Channel(MaybeResolvedChannel),
}

// expects `gift_ids` to be sorted by priority
pub async fn buy_gifts(
    clients: &[Arc<WrappedClient>],
    bot: Arc<Bot>,
    pool: Arc<SqlitePool>,
    gift_ids: Vec<i64>,
    gift_prices_map: Option<&BTreeMap<i64, i64>>,
    limit: Option<u64>,
    dest: &BuyGiftsDestination,
) -> Result<()> {
    let limit = limit.unwrap_or(100);

    let first_client = clients.first().expect("expected at least one client");

    let _dest_peer = match dest {
        BuyGiftsDestination::PeerSelf => InputPeer::PeerSelf,
        BuyGiftsDestination::Channel(channel) => {
            InputPeer::Channel(channel.resolve(first_client).await?)
        }
    };

    let gift_ids: Arc<[_]> = gift_ids.into();
    let gift_prices = get_gift_prices(first_client, &gift_ids, gift_prices_map).await?;

    tracing::debug!(?gift_ids, ?gift_prices, "buy_gifts");

    try_join_all(clients.iter().map(|client| {
        let bot = bot.clone();
        let pool = pool.clone();
        let gift_ids = gift_ids.clone();
        let gift_prices = gift_prices.clone();
        // let dest_peer = dest_peer.clone();

        async move {
            let StarsStatus::Status(status) = client
                .invoke(&GetStarsStatus {
                    peer: InputPeer::PeerSelf,
                })
                .await?;
            tracing::debug!(?status, phone_number = client.phone_number());

            let StarsAmount::Amount(mut stars_amount) = status.balance;

            for (&gift_id, &gift_price) in gift_ids.iter().zip(gift_prices.iter()) {
                for count in 1..=limit {
                    if stars_amount.amount < gift_price {
                        break;
                    }

                    let span = tracing::info_span!(
                        "buy_gift",
                        gift_id,
                        count,
                        phone_number = client.phone_number(),
                    );
                    let _guard = span.enter();

                    let invoice = InputInvoice::StarGift(InputInvoiceStarGift {
                        hide_name: false,
                        include_upgrade: false,
                        // peer: InputPeer::Channel(dest_peer.clone()), // TODO: channel
                        peer: InputPeer::PeerSelf,
                        gift_id,
                        message: None,
                    });

                    let get_payment_form_result = client
                        .invoke(&GetPaymentForm {
                            invoice: invoice.clone(),
                            theme_params: None,
                        })
                        .await;
                    tracing::debug!(?get_payment_form_result);

                    let payment_form = match get_payment_form_result {
                        Ok(t) => t,
                        Err(err) => {
                            tracing::error!(?err, "failed to get payment form");
                            tokio::spawn(
                                notify_gift_buy_status(
                                    bot.clone(),
                                    pool.clone(),
                                    count,
                                    client.phone_number().to_string(),
                                    stars_amount.amount,
                                    gift_id,
                                    GiftBuyStatus::PaymentFormError(err),
                                )
                                .inspect_err(|err| {
                                    tracing::error!(?err, "failed to notify gift buy status")
                                }),
                            );
                            continue;
                        }
                    };

                    let send_stars_form_result = client
                        .invoke(&SendStarsForm {
                            form_id: payment_form.form_id(),
                            invoice,
                        })
                        .await;
                    tracing::debug!(?send_stars_form_result);

                    let status = match send_stars_form_result {
                        Ok(_) => {
                            stars_amount.amount -= gift_price;
                            tracing::debug!(balance = stars_amount.amount, "success");
                            GiftBuyStatus::Success
                        }
                        Err(err) => {
                            tracing::error!(?err, "failed to send stars form");
                            GiftBuyStatus::SendStarsFormError(err)
                        }
                    };

                    tokio::spawn(
                        notify_gift_buy_status(
                            bot.clone(),
                            pool.clone(),
                            count,
                            client.phone_number().to_string(),
                            stars_amount.amount,
                            gift_id,
                            status,
                        )
                        .inspect_err(|err| {
                            tracing::error!(?err, "failed to notify gift buy status")
                        }),
                    );
                }
            }

            Result::<_, Error>::Ok(())
        }
    }))
    .await?;

    Ok(())
}

async fn get_gift_prices(
    first_client: &WrappedClient,
    gift_ids: &[i64],
    gift_prices_map: Option<&BTreeMap<i64, i64>>,
) -> Result<Arc<[i64]>> {
    let gift_prices_map = match gift_prices_map {
        Some(t) => Cow::Borrowed(t),
        None => {
            let result = first_client.invoke(&GetStarGifts { hash: 0 }).await?;

            let gifts = match result {
                StarGifts::Gifts(t) => t,
                StarGifts::NotModified => return Err(Error::UnexpectedNotModified)?,
            };

            Cow::Owned(
                gifts
                    .gifts
                    .into_iter()
                    .filter_map(|gift| match gift {
                        StarGift::Gift(gift) => Some((gift.id, gift.stars)),
                        _ => None,
                    })
                    .collect(),
            )
        }
    };

    gift_ids
        .iter()
        .map(|gift_id| {
            gift_prices_map
                .get(gift_id)
                .copied()
                .ok_or(Error::GiftPriceNotFound(*gift_id))
        })
        .collect::<Result<Arc<[_]>, _>>()
}

#[derive(Debug, Clone)]
pub enum MaybeResolvedChannel {
    Username(String),
    Peer(InputPeerChannel),
}

impl MaybeResolvedChannel {
    pub async fn as_resolved(&self, client: &grammers_client::Client) -> Result<Self> {
        self.resolve(client).await.map(Self::Peer)
    }

    pub async fn resolve(&self, client: &grammers_client::Client) -> Result<InputPeerChannel> {
        Ok(match self {
            Self::Username(username) => {
                let chat = client
                    .resolve_username(username)
                    .await?
                    .ok_or_else(|| Error::ChatNotFound(username.to_string()))?;

                tracing::debug!(username, resolved_chat = ?chat);

                let channel = match chat {
                    Chat::Channel(channel) => channel,
                    _ => return Err(Error::ChatIsNotChannel),
                };

                let access_hash = channel
                    .raw
                    .access_hash
                    .ok_or(Error::ChannelNotAccessible(channel.raw.id))?;

                InputPeerChannel {
                    channel_id: channel.raw.id,
                    access_hash,
                }
            }
            Self::Peer(peer) => peer.clone(),
        })
    }
}
