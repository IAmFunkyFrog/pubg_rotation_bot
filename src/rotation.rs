use std::str;
use scraper::{ElementRef, Html, Selector};
use biome_js_parser::{JsParserOptions, parse};
use biome_js_syntax::{*};
use biome_js_syntax::AnyJsRoot::{*};
use biome_js_syntax::AnyJsStatement::{*};
use biome_js_syntax::AnyJsExpression::{*};
use biome_js_syntax::AnyJsLiteralExpression::JsStringLiteralExpression;
use chrono::{DateTime, Utc, NaiveTime, Weekday};
use chrono::Datelike;

#[derive(Debug, Clone)]
struct Rotation {
    // String is map name, and f32 is map probability
    weeks: Vec<Vec<(String, f32)>>,
}

impl Rotation { 
    // TODO: JSON expected to be formatted in one some special way...
    // need be documented
    fn from_json(weeks_json: &serde_json::Value) -> Option<Self> {
        let weeks_array = weeks_json.as_array()?;
        // Create array of 4 vectors
        let mut weeks_maps: Vec<Vec<(String, f32)>> = Vec::new();
        
        for (week_index, week) in weeks_array.iter().enumerate() {
            let maps_obj = week.get("maps")?.as_object()?;
            let rotation_type = week.get("type")?.as_str()?;
            
            weeks_maps.push(Vec::new());
            for (map_name, map_data) in maps_obj {
                let map_data_obj = map_data.as_object()?;
                let map_name_str = map_name.to_string();
                
                let probability = if rotation_type == "random" {
                    if let Some(selectable) = map_data_obj.get("selectable").and_then(|s| s.as_bool()) {
                        if selectable {
                            100.0
                        } else {
                            // Get probability (can be number or string)
                            map_data_obj.get("probability")
                                .and_then(|p| {
                                    if p.is_number() {
                                        p.as_f64()
                                    } else if p.is_string() {
                                        p.as_str().and_then(|s| s.parse::<f64>().ok())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0.0) as f32
                        }
                    } else {
                        // No selectable field, treat as normal probability
                        map_data_obj.get("probability")
                            .and_then(|p| {
                                if p.is_number() {
                                    p.as_f64()
                                } else if p.is_string() {
                                    p.as_str().and_then(|s| s.parse::<f64>().ok())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0.0) as f32
                    }
                } else {
                    // For non-random types (selection/basic), map always can be chosen, set 100.0
                    // TODO add flag to selectable maps?
                    100.0
                };
                
                weeks_maps[week_index].push((map_name_str, probability));
            }
        }
        
        Some(Rotation { weeks: weeks_maps })
    }
}

#[derive(Debug, Clone)]
pub enum GetRotationError {
    RequestError(String),
    ParseError(String),
    MissingInformation(String),
}

#[derive(Debug)]
pub struct ActualRotationData {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    map_change_time: NaiveTime,
    map_change_weekday: Weekday,
    data: Vec<(String, Rotation)>, // String is region id
}

impl ActualRotationData { 
    fn convert_inner_data(data: &serde_json::Value) -> Option<Vec<(String, Rotation)>> {
        let arr = data.as_array();
        log::info!("Data as array {arr:?}");
        let converted_data = arr?
            .iter()
            .map(|e| {
                log::info!("Try parse...");
                let obj = e.as_object()?;
                let region_id = obj.get("regionId");
                let weeks = obj.get("weeks");
                log::info!("Try parse {region_id:?}, {weeks:?}");
                Some((region_id?.as_str()?.to_string(), Rotation::from_json(weeks?)?))
            });

        let mut v: Vec<(String, Rotation)> = vec![];
        for data_or_none in converted_data {
            v.push(data_or_none?);
        }
        
        Some(v)
    }

    fn from_patch_data(patch_data: &serde_json::Value) -> Option<Self> {
        let content = patch_data.as_object()?;

        log::info!("from_patch_data start");
        let start = content.get("start")?;
        let end = content.get("end")?;
        let map_change_time = content.get("mapChangeTime")?;
        let map_change_weekday = content.get("mapChangeWeekday")?;
        let data = content.get("data")?;

        return Some(ActualRotationData {
            start: DateTime::parse_from_rfc3339(start.as_str()?.strip_prefix("$D")?).ok()?.to_utc(),
            end: DateTime::parse_from_rfc3339(end.as_str()?.strip_prefix("$D")?).ok()?.to_utc(),
            map_change_time: NaiveTime::parse_from_str(map_change_time.as_str()?, "%H:%M").ok()?,
            map_change_weekday: Weekday::try_from(map_change_weekday.as_u64()? as u8 - 1).ok()?,
            data: Self::convert_inner_data(data)?,
        });
    }

    // TODO: JSON expected to be formatted in one some special way...
    // need be documented
    fn from_json(patches_data: &serde_json::Value) -> Option<Self> {
        let now: DateTime<Utc> = Utc::now();
        let arr = patches_data.as_array()?;

        for patch_data in arr {
            if let Some(data) = ActualRotationData::from_patch_data(patch_data) {
                log::info!("Parsed some actual data {data:?}");
                if data.start <= now && now <= data.end {
                    log::info!("Rotation for actual time found!");
                    return Some(data);
                }
            }
        }
        
        None
    }

    pub fn into_rotation_for_moment(self, region: &str, moment: DateTime<Utc>) -> Option<Vec<(String, f32)>> {
        if moment < self.start || moment >= self.end {
            return None;
        }

        // Первая смена карт происходит ровно в следующий map_change_weekday (через неделю после старта)
        // Выставляем время смены на день старта патча
        let base_change = self
            .start
            .date_naive()
            .and_time(self.map_change_time)
            .and_utc();

        // Сдвигаем на 7 дней вперед — это и будет момент перехода на Неделю 2 (индекс 1)
        let first_change = base_change + chrono::Duration::weeks(1);

        let week_index = if moment < first_change {
            0
        } else {
            let elapsed = moment - first_change;
            (elapsed.num_weeks() as usize) + 1
        };

        let rotation_in_region = self
            .data
            .into_iter()
            .find(|(r, _)| r == region)
            .map(|(_, rot)| rot)?;

        let total_weeks = rotation_in_region.weeks.len();
        if total_weeks == 0 {
            return None;
        }

        rotation_in_region.weeks.into_iter().nth(week_index)
    }
}

fn find_rotation_script(html_body: &Html) -> Result<ElementRef<'_>, GetRotationError> {
    let s_selector = Selector::parse("script").unwrap();

    // We expect that on this html page there isscript containing json with data
    for element in html_body.select(&s_selector) {
        let inner_content = element.text().collect::<Vec<&str>>().concat();
        // TODO: make more robust check?
        if inner_content.contains("patches") {
            return Ok(element);
        }
    }

    Err(GetRotationError::ParseError("Not found<script> with json".to_string()))
}

fn extract_json_from_script(rotation_script: &ElementRef<'_>) -> Result<serde_json::Value, GetRotationError> {
    let raw_text = rotation_script.text().collect::<Vec<&str>>().concat();
    log::debug!("Finding JSON in raw text: {raw_text:?}");

    // 1. Ищем начало объекта с патчами
    if let Some(start_pos) = raw_text.find(r#"{\"patches\":["#) {
        let slice = &raw_text[start_pos..];

        // 2. Считаем баланс скобок от начальной '{', чтобы не отрезать на первом патче
        let mut depth = 0;
        let mut end_pos = None;
        let chars: Vec<char> = slice.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '\\' {
                i += 2; // Пропускаем экранированные символы (\", \\ и др.)
                continue;
            }

            match chars[i] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        if let Some(end) = end_pos {
            let raw_object: String = chars[..end].iter().collect();

            // 3. Снимаем экранирование в правильном порядке
            let clean_json = raw_object
                .replace(r#"\\"#, r#"\"#)
                .replace(r#"\""#, r#"""#);

            log::debug!("Clean JSON length: {} chars", clean_json.len());

            // 4. Парсим теперь уже полный и валидный JSON
            match serde_json::from_str::<serde_json::Value>(&clean_json) {
                Ok(json) => {
                    log::debug!("JSON successfully parsed!");
                    return Ok(json);
                }
                Err(e) => {
                    log::error!("Serde parse error: {e}");
                    return Err(GetRotationError::ParseError(format!("Serde parse error: {e}")));
                }
            }
        } else {
            log::warn!("Could not find balanced closing bracket '}}' for root object");
        }
    } else {
        log::warn!("Pattern '{{\\\"patches\\\":[' not found in raw_text");
    }

    Err(GetRotationError::ParseError("Script has unexpected format".to_string()))
}

fn extract_actual_data_from_json(json: &serde_json::Value) -> Result<ActualRotationData, GetRotationError> {
    if let Some(patches_data) = json.as_object().ok_or(GetRotationError::ParseError("Missing \"patches\" field".to_string()))?.get("patches") {
        log::info!("Patches data: {patches_data:?}");
        if let Some(data) = ActualRotationData::from_json(patches_data) {
            return Ok(data);
        }
    }
    
    Err(GetRotationError::ParseError("Unexpected json format!".to_string()))
}

// Gets body of pubg statistics map rotation html and extract rotation
pub fn parse_rotation(html_body_str: &str) -> Result<ActualRotationData, GetRotationError> {
    let html_body = Html::parse_fragment(html_body_str);
    let rotation_script = find_rotation_script(&html_body)?;
    log::debug!("Found rotation script {rotation_script:?}");
    let json = extract_json_from_script(&rotation_script)?;
    extract_actual_data_from_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_html_returns_parse_error() {
        let result = parse_rotation("");
        match result {
            Err(GetRotationError::ParseError(_)) => return,
            _ => panic!("Unexpected result"),
        };
    }

    #[test]
    fn parse_ill_formed_script_returns_parse_error() {
        let result = parse_rotation("<script>random</script>");
        match result {
            Err(GetRotationError::ParseError(_)) => return,
            _ => panic!("Unexpected result"),
        };
    }

    #[test]
    fn parse_script_without_json_returns_parse_error() {
        let result = parse_rotation(BAD_SCRIPT_WITHOUT_JSON);
        match result {
            Err(GetRotationError::ParseError(_)) => return,
            _ => panic!("Unexpected result"),
        };
    }

    #[test]
    fn parse_good_script_simply_works() {
        let result = parse_rotation(GOOD_SCRIPT);
        match result {
            Ok(_) => return,
            Err(err) => panic!("Unexpected result {:?}", err),
        };
    }

    // Note: this fucking large string copied from https://pubgstatistics.com/maprotation
    // Note 2: example of json with data can be found in examples/pretty_json.json file
    const GOOD_SCRIPT: &str = r#"
        <script>self.__next_f.push([1,"3d:I[157818,[\"/_next/static/immutable/chunks/13w2kol3q8q75.js\",\"/_next/static/immutable/chunks/133cexa4k--1s.js\",\"/_next/static/immutable/chunks/3uopvtxpg9_2d.js\",\"/_next/static/immutable/chunks/0s3df1yqd79tq.js\",\"/_next/static/immutable/chunks/1bbeyde0wsux7.js\",\"/_next/static/immutable/chunks/2cu8g0lcmzbm2.js\",\"/_next/static/immutable/chunks/0svmo796nqlw0.js\",\"/_next/static/immutable/chunks/0j8uk5osl0qrc.js\",\"/_next/static/immutable/chunks/2a8ruq5uttyhn.js\",\"/_next/static/immutable/chunks/0g7en015azlk8.js\",\"/_next/static/immutable/chunks/3py135zmn6sik.js\"],\"PatchList\"]\n3e:I[991325,[\"/_next/static/immutable/chunks/13w2kol3q8q75.js\",\"/_next/static/immutable/chunks/133cexa4k--1s.js\",\"/_next/static/immutable/chunks/3uopvtxpg9_2d.js\",\"/_next/static/immutable/chunks/0s3df1yqd79tq.js\",\"/_next/static/immutable/chunks/1bbeyde0wsux7.js\",\"/_next/static/immutable/chunks/2cu8g0lcmzbm2.js\",\"/_next/static/immutable/chunks/0svmo796nqlw0.js\",\"/_next/static/immutable/chunks/0j8uk5osl0qrc.js\",\"/_next/static/immutable/chunks/2a8ruq5uttyhn.js\"],\"IconMark\"]\n34:[[\"$\",\"script\",null,{\"type\":\"application/ld+json\",\"dangerouslySetInnerHTML\":{\"__html\":\"{\\\"@context\\\":\\\"https://schema.org\\\",\\\"@type\\\":\\\"WebPage\\\",\\\"name\\\":\\\"PUBG Map Rotation — Patch 42.3\\\",\\\"description\\\":\\\"Current and upcoming map rotation schedule for PUBG patch 42.3, for both PC and console.\\\",\\\"url\\\":\\\"https://pubgstatistics.com/maprotation\\\",\\\"dateModified\\\":\\\"2026-08-12T00:00:00.000Z\\\"}\"}}],[\"$\",\"$L3d\",null,{\"patches\":[{\"patch\":\"42.1\",\"enabled\":true,\"start\":\"$D2026-06-17T00:00:00.000Z\",\"end\":\"$D2026-07-15T00:00:00.000Z\",\"consoleStart\":\"$D2026-06-25T01:00:00.000Z\",\"consoleEnd\":\"$D2026-07-23T01:00:00.000Z\",\"mapChangeWeekday\":3,\"mapChangeTime\":\"02:00\",\"consoleMapChangeWeekday\":4,\"consoleMapChangeTime\":\"07:00\",\"dataVersion\":2,\"data\":[{\"regionId\":\"AS\",\"forPc\":true,\"forConsole\":false,\"type\":\"selection\",\"weeks\":[{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"},\"Vikendi\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Vikendi\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Paramo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Paramo\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Karakin\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Karakin\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"}}},{\"type\":\"basic\",\"maps\":{\"Rondo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Rondo\"},\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Karakin\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Karakin\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Deston\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Deston\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"}}}]},{\"regionId\":\"SEA\",\"forPc\":true,\"forConsole\":false,\"type\":\"selection\",\"weeks\":[{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"},\"Vikendi\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Vikendi\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Paramo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Paramo\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Karakin\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Karakin\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"}}},{\"type\":\"basic\",\"maps\":{\"Rondo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Rondo\"},\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Karakin\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Karakin\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Deston\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Deston\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"}}}]},{\"regionId\":\"KAKAO\",\"forPc\":true,\"forConsole\":false,\"type\":\"selection\",\"weeks\":[{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Paramo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Paramo\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Vikendi\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Vikendi\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Karakin\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Karakin\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"}}},{\"type\":\"basic\",\"maps\":{\"Rondo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Rondo\"},\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Paramo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Paramo\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Deston\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Deston\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Karakin\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Karakin\"}}}]},{\"regionId\":\"Ranked\",\"forPc\":true,\"forConsole\":true,\"type\":\"ranked\",\"weeks\":[{\"type\":\"random\",\"maps\":{\"Rondo\":{\"type\":\"random\",\"selectable\":false,\"name\":\"Rondo\",\"probability\":25,\"weight\":2},\"Taego\":{\"type\":\"random\",\"selectable\":false,\"name\":\"Taego\",\"probability\":25,\"weight\":2},\"Erangel\":{\"type\":\"random\",\"selectable\":false,\"name\":\"Erangel\",\"probability\":25,\"weight\":2},\"Miramar\":{\"type\":\"random\",\"selectable\":false,\"name\":\"Miramar\",\"probability\":25,\"weight\":2}}}]},{\"regionId\":\"NA\",\"forPc\":true,\"forConsole\":true,\"type\":\"selection\",\"weeks\":[{\"type\":\"basic\",\"maps\":{\"Rondo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Rondo\"},\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Karakin\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Karakin\"},\"Vikendi\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Vikendi\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Deston\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Deston\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"},\"Vikendi\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Vikendi\"}}},{\"type\":\"basic\",\"maps\":{\"Rondo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Rondo\"},\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Paramo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Paramo\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Miramar\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Miramar\"}}},{\"type\":\"basic\",\"maps\":{\"Rondo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Rondo\"},\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Vikendi\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Vikendi\"}}}]},{\"regionId\":\"SA\",\"forPc\":true,\"forConsole\":true,\"type\":\"selection\",\"weeks\":[{\"type\":\"basic\",\"maps\":{\"Rondo\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Rondo\"},\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Sanhok\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Sanhok\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Erangel\"},\"Vikendi\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Vikendi\"}}},{\"type\":\"basic\",\"maps\":{\"Taego\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Taego\"},\"Deston\":{\"type\":\"basic\",\"selectable\":false,\"name\":\"Deston\"},\"Erangel\":{\"type\":\"basic\",\"selectable\":false…</script>
    "#;

    // Note: copy of GOOD_SCRIPT without json with info
    const BAD_SCRIPT_WITHOUT_JSON: &str = r#"
        <script>self.__next_f.push([1])</script>
    "#;
}