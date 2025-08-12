use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use futures::future::try_join_all;
use grammers_client::grammers_tl_types::{
    enums::{
        InputInvoice, InputPeer, StarGift, StarsAmount,
        payments::{StarGifts, StarsStatus},
    },
    functions::payments::{GetPaymentForm, GetStarGifts, GetStarsStatus, SendStarsForm},
    types::InputInvoiceStarGift,
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
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

// expects `gift_ids` to be sorted by priority
pub async fn buy_gifts(
    clients: &[Arc<WrappedClient>],
    bot: Arc<Bot>,
    pool: Arc<SqlitePool>,
    gift_ids: Vec<i64>,
    gift_prices_map: Option<&BTreeMap<i64, i64>>,
    limit: Option<u64>,
) -> Result<()> {
    let first_client = clients.first().expect("expected at least one client");

    let gift_ids: Arc<[_]> = gift_ids.into();
    let gift_prices = get_gift_prices(first_client, &gift_ids, gift_prices_map).await?;

    try_join_all(clients.iter().map(|client| {
        let bot = bot.clone();
        let pool = pool.clone();
        let gift_ids = gift_ids.clone();
        let gift_prices = gift_prices.clone();

        async move {
            let StarsStatus::Status(status) = client
                .invoke(&GetStarsStatus {
                    peer: InputPeer::PeerSelf,
                })
                .await?;

            let StarsAmount::Amount(mut stars_amount) = status.balance;

            for (&gift_id, &gift_price) in gift_ids.iter().zip(gift_prices.iter()) {
                for count in 1..=limit.unwrap_or(u64::MAX) {
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
                        peer: InputPeer::PeerSelf, // TODO: channel
                        gift_id,
                        message: None,
                    });

                    let get_payment_form_result = client
                        .invoke(&GetPaymentForm {
                            invoice: invoice.clone(),
                            theme_params: None,
                        })
                        .await;

                    let payment_form = match get_payment_form_result {
                        Ok(t) => t,
                        Err(err) => {
                            tracing::error!(?err, "failed to get payment form");
                            notify_gift_buy_status(
                                &bot,
                                &pool,
                                count,
                                client.phone_number(),
                                stars_amount.amount,
                                gift_id,
                                GiftBuyStatus::PaymentFormError(err),
                            )
                            .await?;
                            continue;
                        }
                    };

                    let send_stars_form_result = client
                        .invoke(&SendStarsForm {
                            form_id: payment_form.form_id(),
                            invoice,
                        })
                        .await;

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

                    notify_gift_buy_status(
                        &bot,
                        &pool,
                        count,
                        client.phone_number(),
                        stars_amount.amount,
                        gift_id,
                        status,
                    )
                    .await?;
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
