use base64::{engine::general_purpose, Engine as _};
use reqwest;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{Datelike, Local, Timelike};

fn log_time() -> String {
    let now = Local::now();
    format!(
        "{:02}.{:02}.{:04}-{}:{}:{}:{:02}",
        now.day(),
        now.month(),
        now.year(),
        now.hour(),
        now.minute(),
        now.second(),
        now.timestamp_subsec_millis() / 10
    )
}

fn log_info(_step: &str, message: &str) {
    eprintln!("[{}]  {}", log_time(), message);
}

fn log_success(_step: &str, message: &str) {
    eprintln!("[{}]  [ OK ]  {}", log_time(), message);
}

fn log_warning(_step: &str, message: &str) {
    eprintln!("[{}]  {}", log_time(), message);
}

fn log_error(_step: &str, message: &str) {
    eprintln!("[{}]  [ ERROR ]  {}", log_time(), message);
}

const SPOTIFY_URL: &str = "https://open.spotify.com";
const USER_AGENT_API: &str = "https://jnrbsn.github.io/user-agents/user-agents.json";
const VERSIONS_FILE: &str = "versions_web.json";

async fn get_latest_user_agent() -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let response = client.get(USER_AGENT_API).send().await?;
    let user_agents: Vec<String> = response.json().await?;

    Ok(user_agents
        .first()
        .cloned()
        .unwrap_or_else(|| "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string()))
}

fn load_existing_versions() -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
    if Path::new(VERSIONS_FILE).exists() {
        log_info("FILE", &format!("Loading existing {}", VERSIONS_FILE));
        let content = fs::read_to_string(VERSIONS_FILE)?;
        let versions: HashMap<String, Value> = serde_json::from_str(&content)?;
        log_success(
            "FILE",
            &format!("Loaded {} existing versions", versions.len()),
        );
        Ok(versions)
    } else {
        log_warning(
            "FILE",
            &format!("{} not found, treating as new", VERSIONS_FILE),
        );
        Ok(HashMap::new())
    }
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts_a: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let parts_b: Vec<u32> = b.split('.').filter_map(|s| s.parse().ok()).collect();

    parts_b.cmp(&parts_a)
}

fn save_versions(versions: &HashMap<String, Value>) -> Result<(), Box<dyn std::error::Error>> {
    log_info("SORT", "Sorting versions...");

    let mut sorted_keys: Vec<String> = versions.keys().cloned().collect();
    sorted_keys.sort_by(|a, b| compare_versions(a, b));

    log_success("SORT", "Versions sorted");

    let mut json_parts = Vec::new();
    json_parts.push("{\n".to_string());

    for (i, key) in sorted_keys.iter().enumerate() {
        if let Some(value) = versions.get(key) {
            let value_str = serde_json::to_string_pretty(value)?;

            let indented_value: Vec<String> = value_str
                .lines()
                .map(|line| format!("  {}", line))
                .collect();

            json_parts.push(format!("  \"{}\": {}", key, indented_value.join("\n")));

            if i < sorted_keys.len() - 1 {
                json_parts.push(",\n".to_string());
            } else {
                json_parts.push("\n".to_string());
            }
        }
    }

    json_parts.push("}".to_string());

    let json_content = json_parts.join("");
    fs::write(VERSIONS_FILE, json_content)?;
    log_success("FILE", &format!("Saved {} to disk", VERSIONS_FILE));
    Ok(())
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
    let client = reqwest::Client::builder()
        .user_agent(&user_agent)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client.get(SPOTIFY_URL).send().await?;
    let html_content = response.text().await?;

    log_success(
        "HTTP",
        &format!("HTML received ({} bytes)", html_content.len()),
    );

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

            let output = json!({
                "success": false,
                "error": "Base64 content is empty"
            });
            println!("{}", serde_json::to_string(&output)?);

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
            log_success("", &format!("clientVersion: {}", version));
            log_success("", &format!("buildDate: {}", build_date));
            if let Some(bv) = build_version {
                log_success("", &format!("buildVersion: {}", bv));
            }

            let key = version.split('.').take(4).collect::<Vec<_>>().join(".");

            let mut entry = json!({
                "buildDate": build_date,
                "clientVersion": version
            });

            if let Some(url) = web_player_url.as_deref() {
                entry["webPlayer"] = json!(url);
            }
            if let Some(bv) = build_version {
                entry["buildVersion"] = json!(bv);
            }

            log_info("CHECK", "Checking if version is new...");
            match load_existing_versions() {
                Ok(mut versions) => {
                    if versions.contains_key(&key) {
                        log_warning("CHECK", &format!("Version {} already exists", key));
                        let output = json!({
                            "success": true,
                            "is_new": false,
                            "key": key,
                            "message": format!("Version {} already exists", key)
                        });
                        println!("{}", serde_json::to_string(&output)?);
                    } else {
                        log_success("CHECK", &format!("Version {} is NEW!", key));

                        versions.insert(key.clone(), entry.clone());

                        if let Err(e) = save_versions(&versions) {
                            log_error("FILE", &format!("Failed to save versions: {}", e));
                            let output = json!({
                                "success": false,
                                "error": format!("Failed to save versions: {}", e)
                            });
                            println!("{}", serde_json::to_string(&output)?);
                            return Ok(());
                        }

                        let output = json!({
                            "success": true,
                            "is_new": true,
                            "key": key,
                            "data": entry,
                            "message": format!("New version {} detected and saved", version)
                        });
                        println!("{}", serde_json::to_string(&output)?);
                    }
                }
                Err(e) => {
                    log_error("FILE", &format!("Failed to load versions: {}", e));
                    let output = json!({
                        "success": false,
                        "error": format!("Failed to load versions: {}", e)
                    });
                    println!("{}", serde_json::to_string(&output)?);
                }
            }

            log_success("OUTPUT", "JSON output sent to stdout");
        } else {
            log_error(
                "ERROR",
                "Properties 'clientVersion' or 'buildDate' not found!",
            );

            let output = json!({
                "success": false,
                "error": "clientVersion or buildDate not found"
            });
            println!("{}", serde_json::to_string(&output)?);
        }
    } else {
        log_error("FAIL", "Tag 'appServerConfig' not found in HTML!");

        let output = json!({
            "success": false,
            "error": "appServerConfig tag not found"
        });
        println!("{}", serde_json::to_string(&output)?);
    }

    Ok(())
}
