use std::io::{Read, Write};

use anyhow::Result;
use dialoguer::Input;
use flate2::bufread::GzDecoder;
use grammers_client::{
    Client,
    grammers_tl_types::{
        enums::{InputFileLocation, payments::StarGifts, upload},
        functions::{payments::GetStarGifts, upload::GetFile},
        types::InputDocumentFileLocation,
    },
    session::Session,
};
use image::{ImageFormat, RgbaImage};
use rlottie::{Animation, Surface};
use serde::Deserialize;
use tch::CModule;

#[derive(Deserialize)]
struct Config {
    api_id: i32,
    api_hash: String,
    phone_number: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config: Config = envy::from_env()?;

    // let module = CModule::load("encoder")?;

    let session_path = "sessions/demo.session";

    let client = Client::connect(grammers_client::Config {
        session: Session::load_file_or_create(session_path)?,
        api_id: config.api_id,
        api_hash: config.api_hash,
        params: Default::default(),
    })
    .await?;

    let is_authorized = client.is_authorized().await?;
    tracing::debug!(is_authorized);

    if !is_authorized {
        let login_token = client.request_login_code(&config.phone_number).await?;

        let login_code: String = Input::new().with_prompt("Enter Login Code").interact()?;

        let user = client.sign_in(&login_token, &login_code).await?;
        tracing::debug!(?user);

        client.sync_update_state();
        client.session().save_to_file(session_path)?;
    }

    let response = client.invoke(&GetStarGifts { hash: 0 }).await?;

    if let StarGifts::Gifts(gifts) = response {
        tracing::debug!(gift = ?gifts.gifts.last());
    }

    let request = GetFile {
        precise: true,
        cdn_supported: false,
        location: InputFileLocation::InputDocumentFileLocation(InputDocumentFileLocation {
            id: 5330191715850541636,
            access_hash: 3921228561330411875,
            file_reference: vec![
                0, 104, 152, 172, 174, 230, 186, 218, 83, 75, 125, 217, 165, 58, 177, 245, 249, 35,
                64, 161, 52,
            ],
            thumb_size: "".to_string(),
        }),
        offset: 0,
        limit: 1024 * 1023,
    };
    let response = client.invoke(&request).await?;
    tracing::debug!(?response);

    let upload::File::File(file) = response else {
        panic!("not a file");
    };

    // let mut out = std::fs::File::create("output/thumb.png")?;
    // out.write_all(&file.bytes)?;

    let now = std::time::Instant::now();

    let mut gz = GzDecoder::new(&file.bytes[..]);

    let mut animation_data = vec![];
    gz.read_to_end(&mut animation_data)?;

    tracing::debug!(elapsed = ?now.elapsed());

    let mut animation =
        Animation::from_data(animation_data, "5330191715850541636", "").expect("animation is None");

    let size = animation.size();

    // tracing::debug!(animation_size = ?size);

    let mut surface = Surface::new(size);

    animation.render(0, &mut surface);

    // tracing::debug!(len = surface.data_as_bytes().len());

    let mut bgra_data = surface.into_data();
    for bgra in &mut bgra_data {
        // bgra -> rgba
        std::mem::swap(&mut bgra.r, &mut bgra.b);
    }

    let data = {
        let ptr = bgra_data.as_mut_ptr() as *mut u8;
        let len = bgra_data.len() * 4;
        let capacity = bgra_data.capacity() * 4;

        std::mem::forget(bgra_data);

        unsafe { Vec::from_raw_parts(ptr, len, capacity) }
    };

    let image =
        RgbaImage::from_vec(size.width as u32, size.height as u32, data).expect("image is None");

    tracing::debug!(elapsed = ?now.elapsed());

    image.save_with_format("./output/frame.png", ImageFormat::Png)?;

    // let invoice = InputInvoice::StarGift(InputInvoiceStarGift {
    //     hide_name: false,
    //     include_upgrade: false,
    //     peer: InputPeer::PeerSelf,
    //     gift_id: 5170145012310081615,
    //     message: None,
    // });

    // let payment_form = client
    //     .invoke(&GetPaymentForm {
    //         invoice: invoice.clone(),
    //         theme_params: None,
    //     })
    //     .await?;
    // tracing::debug!(?payment_form);

    // let result = client
    //     .invoke(&SendStarsForm {
    //         form_id: payment_form.form_id(),
    //         invoice,
    //     })
    //     .await?;
    // tracing::debug!(?result);

    Ok(())
}
