mod rotation;

use teloxide::{dispatching::dialogue::GetChatId, prelude::*, types::{InlineKeyboardButton, InlineKeyboardMarkup, ReplyMarkup}};
use std::{collections::HashMap, str};
use reqwest;
use rotation::{*};

fn prettify_rotation(region: &str, rotation: &Vec<(String, f32)>) -> String {
    let pretty_map_names = std::collections::HashMap::from([
        ("Erangel", "🏝️ Erangel".to_string()),
        ("Miramar", "☀️ Miramar".to_string()),
        ("Sanhok", "🏔️ Sanhok".to_string()),
        ("Taego", "🏰 Taego".to_string()),
        ("Vikendi", "❄️ Vikendi".to_string()),
        ("Karakin", "🏙️ Karakin".to_string()),
        ("Deston", "🏠 Deston".to_string()),
        ("Paramo", "🏔️ Paramo".to_string()),
        ("Rondo", "🌲 Rondo".to_string()),
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

#[cfg(test)]
mod emoji_tests {
    use super::*;

    #[test]
    fn test_erangel_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Erangel".to_string(), 100.0)]);
        assert!(map_names.contains("🏝️ Erangel"));
    }

    #[test]
    fn test_miramar_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Miramar".to_string(), 100.0)]);
        assert!(map_names.contains("☀️ Miramar"));
    }

    #[test]
    fn test_sanhok_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Sanhok".to_string(), 100.0)]);
        assert!(map_names.contains("🏔️ Sanhok"));
    }

    #[test]
    fn test_taego_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Taego".to_string(), 100.0)]);
        assert!(map_names.contains("🏰 Taego"));
    }

    #[test]
    fn test_vikendi_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Vikendi".to_string(), 100.0)]);
        assert!(map_names.contains("❄️ Vikendi"));
    }

    #[test]
    fn test_karakin_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Karakin".to_string(), 100.0)]);
        assert!(map_names.contains("🏙️ Karakin"));
    }

    #[test]
    fn test_deston_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Deston".to_string(), 100.0)]);
        assert!(map_names.contains("🏠 Deston"));
    }

    #[test]
    fn test_paramo_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Paramo".to_string(), 100.0)]);
        assert!(map_names.contains("🏔️ Paramo"));
    }

    #[test]
    fn test_rondo_emoji() {
        let map_names = prettify_rotation("Test", &vec![("Rondo".to_string(), 100.0)]);
        assert!(map_names.contains("🌲 Rondo"));
    }

    #[test]
    fn test_all_emojis_are_in_output() {
        let rotation = vec![
            ("Erangel".to_string(), 10.0),
            ("Miramar".to_string(), 20.0),
            ("Sanhok".to_string(), 30.0),
            ("Taego".to_string(), 40.0),
            ("Vikendi".to_string(), 50.0),
            ("Karakin".to_string(), 60.0),
            ("Deston".to_string(), 70.0),
            ("Paramo".to_string(), 80.0),
            ("Rondo".to_string(), 90.0),
        ];
        let output = prettify_rotation("Test", &rotation);
        
        assert!(output.contains("🏝️ Erangel"));
        assert!(output.contains("☀️ Miramar"));
        assert!(output.contains("🏔️ Sanhok"));
        assert!(output.contains("🏰 Taego"));
        assert!(output.contains("❄️ Vikendi"));
        assert!(output.contains("🏙️ Karakin"));
        assert!(output.contains("🏠 Deston"));
        assert!(output.contains("🏔️ Paramo"));
        assert!(output.contains("🌲 Rondo"));
    }

    #[test]
    fn test_map_names_preserved_in_emoji_strings() {
        let map_names = prettify_rotation("Test", &vec![("Erangel".to_string(), 100.0)]);
        assert!(map_names.contains("Erangel"));
        
        let map_names = prettify_rotation("Test", &vec![("Miramar".to_string(), 100.0)]);
        assert!(map_names.contains("Miramar"));
        
        let map_names = prettify_rotation("Test", &vec![("Sanhok".to_string(), 100.0)]);
        assert!(map_names.contains("Sanhok"));
        
        let map_names = prettify_rotation("Test", &vec![("Taego".to_string(), 100.0)]);
        assert!(map_names.contains("Taego"));
        
        let map_names = prettify_rotation("Test", &vec![("Vikendi".to_string(), 100.0)]);
        assert!(map_names.contains("Vikendi"));
        
        let map_names = prettify_rotation("Test", &vec![("Karakin".to_string(), 100.0)]);
        assert!(map_names.contains("Karakin"));
        
        let map_names = prettify_rotation("Test", &vec![("Deston".to_string(), 100.0)]);
        assert!(map_names.contains("Deston"));
        
        let map_names = prettify_rotation("Test", &vec![("Paramo".to_string(), 100.0)]);
        assert!(map_names.contains("Paramo"));
        
        let map_names = prettify_rotation("Test", &vec![("Rondo".to_string(), 100.0)]);
        assert!(map_names.contains("Rondo"));
    }
}
