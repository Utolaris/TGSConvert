use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde::Deserialize;

const TELEGRAM_BOT_TOKEN: &str = "***REMOVED***";
const API_BASE: &str = "https://api.telegram.org";

#[derive(Clone, Debug)]
pub struct TelegramDownloadOptions {
    pub link_or_name: String,
    pub output_directory: PathBuf,
    pub threads: usize,
}

#[derive(Clone, Debug)]
pub struct TelegramDownloadReport {
    pub set_name: String,
    pub title: String,
    pub files: usize,
    pub output_directory: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StickerSet {
    name: String,
    title: String,
    #[serde(default)]
    sticker_type: String,
    stickers: Vec<Sticker>,
}

#[derive(Clone, Debug, Deserialize)]
struct Sticker {
    file_id: String,
    file_unique_id: String,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    is_animated: bool,
    #[serde(default)]
    is_video: bool,
}

#[derive(Debug, Deserialize)]
struct TelegramFile {
    file_path: String,
}

#[derive(Clone, Debug)]
struct DownloadItem {
    file_id: String,
    unique_id: String,
    emoji: Option<String>,
    is_animated: bool,
    is_video: bool,
}

pub fn download_sticker_set(options: &TelegramDownloadOptions) -> Result<TelegramDownloadReport> {
    if options.threads == 0 {
        bail!("--threads must be at least 1");
    }

    let requested_name = parse_sticker_set_name(&options.link_or_name)?;
    fs::create_dir_all(&options.output_directory).with_context(|| {
        format!(
            "failed to create output directory {}",
            options.output_directory.display()
        )
    })?;

    let client = Client::builder()
        .user_agent("tgs-convert Telegram sticker downloader")
        .build()
        .context("failed to create Telegram HTTP client")?;
    let set = get_sticker_set(&client, &requested_name)?;
    if set.stickers.is_empty() {
        bail!("Telegram sticker set {} is empty", set.name);
    }

    let include_emoji = set.sticker_type == "custom_emoji";
    let items = set
        .stickers
        .into_iter()
        .map(|sticker| DownloadItem {
            file_id: sticker.file_id,
            unique_id: sticker.file_unique_id,
            emoji: include_emoji.then_some(sticker.emoji).flatten(),
            is_animated: sticker.is_animated,
            is_video: sticker.is_video,
        })
        .collect::<Vec<_>>();
    let file_count = items.len();
    let workers = options.threads.min(file_count);
    let cancel = Arc::new(AtomicBool::new(false));
    install_cancel_handler(Arc::clone(&cancel))?;
    let next_index = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let first_error = Mutex::new(None::<String>);

    eprintln!(
        "Downloading {} sticker(s) from {} with {} worker(s)",
        file_count, set.name, workers
    );

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let client = client.clone();
            let output_directory = &options.output_directory;
            let items = &items;
            let next_index = &next_index;
            let completed = &completed;
            let cancel = Arc::clone(&cancel);
            let first_error = &first_error;
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let index = next_index.fetch_add(1, Ordering::AcqRel);
                    if index >= items.len() {
                        break;
                    }

                    if let Err(error) = download_one(&client, &items[index], output_directory) {
                        cancel.store(true, Ordering::Release);
                        let mut slot = first_error.lock().expect("download error mutex poisoned");
                        if slot.is_none() {
                            *slot = Some(format!("sticker {}: {error:#}", index + 1));
                        }
                        break;
                    }

                    let done = completed.fetch_add(1, Ordering::AcqRel) + 1;
                    eprintln!("Downloaded {done}/{file_count}");
                }
            });
        }
    });

    if let Some(error) = first_error
        .lock()
        .expect("download error mutex poisoned")
        .take()
    {
        return Err(anyhow!(error));
    }
    if cancel.load(Ordering::Acquire) {
        bail!("download cancelled");
    }

    Ok(TelegramDownloadReport {
        set_name: set.name,
        title: set.title,
        files: file_count,
        output_directory: options.output_directory.clone(),
    })
}

pub fn parse_sticker_set_name(link_or_name: &str) -> Result<String> {
    let trimmed = link_or_name.trim();
    let candidate = if let Some((_, rest)) = trimmed.split_once("://") {
        let without_query = rest.split(['?', '#']).next().unwrap_or_default();
        let mut parts = without_query.split('/').filter(|part| !part.is_empty());
        let host = parts.next().unwrap_or_default().to_ascii_lowercase();
        let kind = parts.next().unwrap_or_default().to_ascii_lowercase();
        let name = parts.next().unwrap_or_default();
        if (host != "t.me" && host != "www.t.me") || (kind != "addstickers" && kind != "addemoji") {
            bail!("Telegram link must use t.me/addstickers/<name> or t.me/addemoji/<name>");
        }
        if parts.next().is_some() || name.is_empty() {
            bail!("Telegram link must contain exactly one sticker-set name");
        }
        name
    } else {
        trimmed
    };

    if candidate.is_empty()
        || candidate.len() > 64
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        bail!("invalid Telegram sticker-set name");
    }
    Ok(candidate.to_owned())
}

fn get_sticker_set(client: &Client, name: &str) -> Result<StickerSet> {
    telegram_api(client, "getStickerSet", [("name", name)])
        .with_context(|| format!("failed to fetch Telegram sticker set {name}"))
}

fn get_file(client: &Client, file_id: &str) -> Result<TelegramFile> {
    telegram_api(client, "getFile", [("file_id", file_id)])
        .context("failed to fetch Telegram file metadata")
}

fn telegram_api<T>(client: &Client, method: &str, query: [(&str, &str); 1]) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let url = format!("{API_BASE}/bot{TELEGRAM_BOT_TOKEN}/{method}");
    let response = client
        .get(url)
        .query(&query)
        .send()
        .with_context(|| format!("Telegram API request {method} failed"))?
        .error_for_status()
        .with_context(|| format!("Telegram API request {method} returned an HTTP error"))?;
    let response = response
        .json::<ApiResponse<T>>()
        .with_context(|| format!("Telegram API request {method} returned invalid JSON"))?;
    if !response.ok {
        bail!(
            "Telegram API request {method} was rejected: {}",
            response
                .description
                .unwrap_or_else(|| "no description".to_owned())
        );
    }
    response
        .result
        .ok_or_else(|| anyhow!("Telegram API request {method} returned no result"))
}

fn download_one(client: &Client, item: &DownloadItem, output_directory: &Path) -> Result<()> {
    let metadata = get_file(client, &item.file_id)?;
    let extension =
        extension_from_path(&metadata.file_path).unwrap_or_else(|| fallback_extension(item));
    let filename = filename(item, extension);
    let destination = output_directory.join(filename);
    let partial_destination = destination.with_extension(format!("{extension}.part"));
    let url = format!(
        "{API_BASE}/file/bot{TELEGRAM_BOT_TOKEN}/{}",
        metadata.file_path
    );

    let mut response = client
        .get(url)
        .send()
        .context("Telegram file download request failed")?
        .error_for_status()
        .context("Telegram file download returned an HTTP error")?;
    let mut partial = File::create(&partial_destination)
        .with_context(|| format!("failed to create {}", partial_destination.display()))?;
    io::copy(&mut response, &mut partial)
        .with_context(|| format!("failed to write {}", partial_destination.display()))?;
    drop(partial);
    fs::rename(&partial_destination, &destination)
        .with_context(|| format!("failed to finalize {}", destination.display()))?;
    Ok(())
}

fn extension_from_path(file_path: &str) -> Option<&str> {
    let extension = Path::new(file_path).extension()?.to_str()?;
    (extension.len() <= 10 && extension.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then_some(extension)
}

fn fallback_extension(item: &DownloadItem) -> &'static str {
    if item.is_animated {
        "tgs"
    } else if item.is_video {
        "webm"
    } else {
        "webp"
    }
}

fn filename(item: &DownloadItem, extension: &str) -> String {
    let emoji = item.emoji.as_deref().map(clean_emoji).unwrap_or_default();
    let stem = if emoji.is_empty() {
        item.unique_id.clone()
    } else {
        format!("{emoji}_{}", item.unique_id)
    };
    format!("{stem}.{extension}")
}

fn clean_emoji(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            *character != '\u{fe0f}'
                && !character.is_control()
                && !matches!(
                    *character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        })
        .collect()
}

fn install_cancel_handler(cancel: Arc<AtomicBool>) -> Result<()> {
    ctrlc::set_handler(move || {
        cancel.store(true, Ordering::Release);
        eprintln!("\nCancellation requested; stopping active downloads.");
    })
    .map_err(|error| anyhow!("failed to install Ctrl-C handler: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{DownloadItem, clean_emoji, filename, parse_sticker_set_name};

    #[test]
    fn parses_sticker_and_emoji_links() {
        assert_eq!(
            parse_sticker_set_name("https://t.me/addstickers/HotCherry?startapp=x").unwrap(),
            "HotCherry"
        );
        assert_eq!(
            parse_sticker_set_name("https://t.me/addemoji/Custom_Emoji/").unwrap(),
            "Custom_Emoji"
        );
    }

    #[test]
    fn rejects_non_pack_links_and_unsafe_names() {
        assert!(parse_sticker_set_name("https://t.me/example").is_err());
        assert!(parse_sticker_set_name("../not-a-pack").is_err());
    }

    #[test]
    fn custom_emoji_filename_matches_desktop_pattern() {
        let item = DownloadItem {
            file_id: "unused".to_owned(),
            unique_id: "unique".to_owned(),
            emoji: Some("😀\u{fe0f}".to_owned()),
            is_animated: false,
            is_video: false,
        };
        assert_eq!(filename(&item, "tgs"), "😀_unique.tgs");
        assert_eq!(clean_emoji("a/b"), "ab");
    }
}
