mod rotation;

use teloxide::{dispatching::dialogue::GetChatId, prelude::*, types::{InlineKeyboardButton, InlineKeyboardMarkup, ReplyMarkup}};
use std::{collections::HashMap, str};
use reqwest;
use rotation::{*};

fn prettify_rotation(region: &str, rotation: &Vec<(String, f32)>) -> String {
    let pretty_map_names = std::collections::HashMap::from([
        ("Erangel", "🏝️ Erangel".to_string()),
        ("Miramar", "☀️ Miramar".to_string()),
    ]);

    rotation.iter()
            .map(|e| (pretty_map_names.get(e.0.as_str()).unwrap_or(&e.0), e.1))
            .fold(format!("Ротация на {} сервере:\n", region), |acc, e| acc + &format!("{} : {} %\n", e.0, e.1))
}

async fn fetch_url(url: &str) -> Result<String, reqwest::Error> {
    let response = reqwest::get(url).await?;
    response.text().await
}

async fn get_rotation(map_rotations_url: &str, region: &str) -> Result<Vec<(String, f32)>, GetRotationError> {
    let pubg_statistics_body = fetch_url(map_rotations_url).await;
    
    let rotation_data = match pubg_statistics_body {
        Ok(body) => parse_rotation(&body),
        Err(err) => Err(GetRotationError::RequestError(err.to_string())),
    }?;

    rotation_data.into_rotation_for_moment(region, chrono::Utc::now())
                 .ok_or(GetRotationError::MissingInformation(format!("No available info for {} and {:?}", region, chrono::Utc::now())))
}

async fn show_inline_keyboard(bot: &Bot, msg: &Message) -> ResponseResult<()> {
    // Создаем inline-кнопки разных типов
    let show_ru_rotation_button = InlineKeyboardButton::callback("Показать ротацию (RU сервер)", "RU");

    // Формируем клавиатуру (кнопки в рядах)
    let keyboard = InlineKeyboardMarkup::new([
        [show_ru_rotation_button], // Первый ряд
    ]);

    // Отправляем сообщение с inline-клавиатурой
    bot.send_message(msg.chat.id, "Выберите действие:")
        .reply_markup(ReplyMarkup::InlineKeyboard(keyboard))
        .await?;

    Ok(())
}

async fn handle_callback_query(bot: Bot, q: CallbackQuery) -> ResponseResult<()> {
    let map_rotation_url = "https://pubgstatistics.com/maprotation";
    if let Some(chat_id) = q.chat_id() {
        if let Some(region) = q.data {
            bot.send_message(chat_id, format!("Запрашиваем ротацию на {}", map_rotation_url)).await?;
            if let Ok(rotation) = get_rotation(map_rotation_url, region.as_str()).await {
                bot.send_message(chat_id, prettify_rotation(region.as_str(), &rotation)).await?;
            } else {
                bot.send_message(chat_id, format!("Проблемы с доступом к {}", map_rotation_url)).await?;
            }
        }
        bot.answer_callback_query(q.id).await?;
    }
    Ok(())
}

async fn handle_message(bot: Bot, msg: Message) -> ResponseResult<()> {
    log::info!("Got message... {msg:?}");
    if let Some(text) = msg.text() {
        match text {
            _ => {
                show_inline_keyboard(&bot, &msg).await?;
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    let bot = Bot::from_env();
    log::info!("Starting bot... {bot:?}");

    let handler = dptree::entry()
        .branch(Update::filter_callback_query().endpoint(handle_callback_query))
        .branch(Update::filter_message().endpoint(handle_message));

    Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
