use anyhow::Result;
use dialoguer::Input;
use grammers_client::{
    Client,
    grammers_tl_types::{
        enums::{InputFileLocation, payments::StarGifts, upload},
        functions::{payments::GetStarGifts, upload::GetFile},
        types::InputDocumentFileLocation,
    },
    session::Session,
};
use image::{EncodableLayout, ImageFormat, imageops::FilterType};
use serde::Deserialize;
use tch::{CModule, Kind, Tensor};

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

    let model = CModule::load("encoder")?;

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
            thumb_size: "m".to_string(),
        }),
        offset: 0,
        limit: 1024 * 1023,
    };
    let response = client.invoke(&request).await?;
    // tracing::debug!(?response);

    let upload::File::File(file) = response else {
        panic!("not a file");
    };

    let image = image::load_from_memory_with_format(&file.bytes, ImageFormat::WebP)?;
    image.save_with_format("output/input.png", ImageFormat::Png)?;

    let resized = image.resize(224, 224, FilterType::CatmullRom);
    let rgb = resized.to_rgb8();

    dbg!(rgb.as_bytes().len());
    rgb.save_with_format("output/rgb.png", ImageFormat::Png)?;

    let input = Tensor::from_data_size(rgb.as_bytes(), &[224, 224, 3], Kind::Uint8)
        .to_kind(Kind::Float)
        .divide_scalar(255)
        .permute([2, 0, 1])
        .unsqueeze(0);

    let mean = Tensor::from_slice(&[0.485_f32, 0.456, 0.406]).view([1, 3, 1, 1]);
    let std = Tensor::from_slice(&[0.229_f32, 0.224, 0.225]).view([1, 3, 1, 1]);
    let input = (input - mean) / std;

    input.save("output/input.pt")?;

    // dbg!(&input);

    let output = model.forward_ts(&[input])?;

    let output: Vec<f32> = output.view(-1).try_into()?;
    dbg!(output);

    Ok(())
}
