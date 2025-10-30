use base64::{engine::general_purpose, Engine as _};
use reqwest;
use scraper::{Html, Selector};
use serde_json::Value;
use std::fs;

use chrono::{Datelike, Local, Timelike};
fn log_time() -> String {
    let now = Local::now();
    format!(
        "{:02}.{:02}.{:04}-{}:{}:{}:{}",
        now.day(),
        now.month(),
        now.year(),
        now.hour(),
        now.minute(),
        now.second(),
        now.timestamp_subsec_millis()
    )
}

fn log_info(_step: &str, message: &str) {
    println!("[{}]  {}", log_time(), message);
}

fn log_success(_step: &str, message: &str) {
    println!("[{}]  [ OK ]  {}", log_time(), message);
}

fn log_warning(_step: &str, message: &str) {
    println!("[{}]  {}", log_time(), message);
}

fn log_error(_step: &str, message: &str) {
    println!("[{}]  [ ERROR ]  {}", log_time(), message);
}

const SPOTIFY_URL: &str = "https://open.spotify.com";
const USER_AGENT_API: &str = "https://jnrbsn.github.io/user-agents/user-agents.json";

async fn get_latest_user_agent() -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client.get(USER_AGENT_API).send().await?;
    let user_agents: Vec<String> = response.json().await?;

    Ok(user_agents
        .first()
        .cloned()
        .unwrap_or_else(|| "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    log_info("INIT", "Starting ...");

    log_info("NET", "Getting actual User-Agent...");
    let user_agent = get_latest_user_agent().await.unwrap_or_else(|_| {
        log_warning("NET", "Failed to get UA from API, using fallback");
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string()
    });

    log_success("NET", &format!("User-Agent set: {}", user_agent));

    log_info("HTTP", &format!("Sending request to {}", SPOTIFY_URL));
    let client = reqwest::Client::builder().user_agent(&user_agent).build()?;

    let response = client.get(SPOTIFY_URL).send().await?;
    let html_content = response.text().await?;

    log_success(
        "HTTP",
        &format!("HTML received ({} bytes)", html_content.len()),
    );

    fs::write("spotify_page.html", &html_content)?;
    log_info("FILE", "HTML saved to spotify_page.html");

    log_info("PARSE", "Parsing HTML document...");
    let document = Html::parse_document(&html_content);

    log_info("SEARCH", "Method 1: Scraper...");
    let selectors = vec![
        r#"script[id="appServerConfig"][type="text/plain"]"#,
        r#"script[id="appServerConfig"]"#,
        r#"script#appServerConfig"#,
    ];

    let mut base64_string: Option<String> = None;

    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                log_success("SCRAPER", &format!("Tag found: {}", selector_str));
                base64_string = Some(element.text().collect::<String>().trim().to_string());
                break;
            }
        }
    }

    if base64_string.is_none() {
        log_info("SEARCH", "Method 2: Regex...");
        use regex::Regex;
        let re = Regex::new(r#"<script[^>]*id="appServerConfig"[^>]*>([^<]+)</script>"#)?;
        if let Some(caps) = re.captures(&html_content) {
            log_success("REGEX", "Tag found via regex");
            base64_string = Some(caps.get(1).unwrap().as_str().trim().to_string());
        }
    }

    log_info("SEARCH", "Searching for web-player ...");
    let selector_js = Selector::parse("script[src]").expect("Selector creation error");
    let mut web_player_url: Option<String> = None;
    for element in document.select(&selector_js) {
        if let Some(src) = element.value().attr("src") {
            if src.contains("web-player") && src.ends_with(".js") {
                web_player_url = Some(src.to_string());
                log_success("FOUND", &format!("Web-player: {}", src));
                break;
            }
        }
    }

    if let Some(base64_str) = base64_string {
        if base64_str.is_empty() {
            log_error("ERROR", "Tag found, but content is empty!");
            return Ok(());
        }

        log_success(
            "B64",
            &format!("Base64 string found ({} characters)", base64_str.len()),
        );

        log_info("DECODE", "Decoding Base64...");
        let decoded_bytes = general_purpose::STANDARD.decode(&base64_str)?;
        let decoded_json = String::from_utf8(decoded_bytes)?;

        log_info("JSON", "Parsing JSON data...");
        let json_object: Value = serde_json::from_str(&decoded_json)?;

        let version = json_object.get("clientVersion").and_then(|v| v.as_str());
        let build_date = json_object.get("buildDate").and_then(|v| v.as_str());
        let build_version = json_object.get("buildVersion").and_then(|v| v.as_str());

        if let (Some(version), Some(build_date)) = (version, build_date) {
            log_success("", "Version data extracted");
            if !version.is_empty() {
                log_success("", &format!("clientVersion: {}", version));
            }
            if !build_date.is_empty() {
                log_success("", &format!("buildDate: {}", build_date));
            }
            if let Some(bv) = build_version {
                if !bv.is_empty() {
                    log_success("", &format!("buildVersion: {}", bv));
                }
            }

            log_info("SAVE", "Saving data to JSON file...");
            add_version_to_json(
                version,
                &build_date,
                web_player_url.as_deref(),
                build_version,
            )?;
            log_success("OK", "Data successfully added to version_web_spotify.json");
        } else {
            log_error(
                "ERROR",
                "Properties 'clientVersion' or 'buildDate' not found!",
            );
        }
    } else {
        log_error("FAIL", "Tag 'appServerConfig' not found in HTML!");
        log_warning("DEBUG", "Check spotify_page.html for diagnostics");
    }

    Ok(())
}

fn add_version_to_json(
    version: &str,
    build_date: &str,
    web_player_url: Option<&str>,
    build_version: Option<&str>,
) -> Result<(), std::io::Error> {
    use serde_json::{json, Map, Value};
    let file_path = "version_web_spotify.json";

    let mut versions_map = if let Ok(content) = fs::read_to_string(file_path) {
        serde_json::from_str::<Value>(&content)
            .unwrap_or_else(|_| json!({}))
            .as_object()
            .cloned()
            .unwrap_or_else(Map::new)
    } else {
        Map::new()
    };

    let key = version.split('.').take(4).collect::<Vec<_>>().join(".");

    let mut entry = json!({
        "buildDate": build_date,
        "clientVersion": version
    });
    if let Some(url) = web_player_url {
        entry["webPlayer"] = json!(url);
    }
    if let Some(bv) = build_version {
        entry["buildVersion"] = json!(bv);
    }

    versions_map.insert(key.to_string(), entry);

    let json_data = Value::Object(versions_map);
    let json_str = serde_json::to_string_pretty(&json_data).unwrap();
    fs::write(file_path, json_str)?;
    Ok(())
}
