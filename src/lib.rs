use std::{fs, path::{Path, PathBuf}};
use tauri::{RunEvent, Manager, Runtime};
use tauri::plugin::{Builder as PluginBuilder, TauriPlugin};
use winit::event::WindowEvent as WinitWindowEvent;

use serde::Serialize;

#[derive(Serialize)]
struct LinkMetadata {
    url: String,
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
    favicon: Option<String>,
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    PluginBuilder::new("linkdrop")
        .on_event(|app_handle, event| {
            if let RunEvent::WindowEvent { event, .. } = event {
                if let WinitWindowEvent::DroppedFile(path) = event {
                    let path_buf = PathBuf::from(path.clone());
                    let app = app_handle.clone();
                    std::thread::spawn(move || {
                        if let Some(meta) = handle_dropped_file(&path_buf) {
                            let _ = app.emit_all("link-dropped", meta);
                        }
                    });
                }
            }
            Ok(())
        })
        .build()
}

fn handle_dropped_file(path: &Path) -> Option<LinkMetadata> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "webloc" || ext == "url" {
        if let Ok(url) = parse_shortcut(path, &ext) {
            match fetch_metadata(&url) {
                Ok(meta) => return Some(meta),
                Err(_) => {
                    return Some(LinkMetadata {
                        url,
                        title: None,
                        description: None,
                        image: None,
                        favicon: None,
                    });
                }
            }
        }
    }
    None
}

fn parse_shortcut(path: &Path, ext: &str) -> Result<String, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    if ext == "url" {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("URL=") {
                return Ok(rest.trim().to_string());
            }
        }
    } else if ext == "webloc" {
        // crude XML/plist parsing for <string>URL</string>
        if let Some(start) = content.find("<string>") {
            let after = start + "<string>".len();
            if let Some(end) = content[after..].find("</string>") {
                let url = &content[after..after + end];
                return Ok(url.trim().to_string());
            }
        }
    }
    Err("Failed to parse shortcut file".into())
}

fn fetch_metadata(url: &str) -> Result<LinkMetadata, Box<dyn std::error::Error>> {
    let resp = reqwest::blocking::get(url)?;
    let base_url = resp.url().clone();
    let html = resp.text()?;

    let document = scraper::Html::parse_document(&html);

    // Title
    let mut title = None;
    if let Some(elem) = document.select(&scraper::Selector::parse("title").unwrap()).next() {
        let text: String = elem.text().collect();
        if !text.trim().is_empty() {
            title = Some(text.trim().to_string());
        }
    }
    if title.is_none() {
        if let Some(meta) = document.select(&scraper::Selector::parse(r#"meta[property=\"og:title\"]"#).unwrap()).next() {
            if let Some(content) = meta.value().attr("content") {
                title = Some(content.to_string());
            }
        }
    }

    // Description
    let mut description = None;
    for sel in &[r#"meta[name=\"description\"]"#, r#"meta[property=\"og:description\"]"#] {
        if let Some(meta) = document.select(&scraper::Selector::parse(sel).unwrap()).next() {
            if let Some(content) = meta.value().attr("content") {
                description = Some(content.to_string());
                break;
            }
        }
    }

    // Image
    let image = document
        .select(&scraper::Selector::parse(r#"meta[property=\"og:image\"]"#).unwrap())
        .next()
        .and_then(|m| m.value().attr("content"))
        .map(|s| s.to_string());

    // Favicon
    let favicon = document
        .select(&scraper::Selector::parse(r#"link[rel~=\"icon\"]"#).unwrap())
        .next()
        .and_then(|l| l.value().attr("href"))
        .map(|href| {
            if href.starts_with("http") || href.starts_with("//") {
                href.to_string()
            } else {
                base_url.join(href).map(|u| u.to_string()).unwrap_or_else(|_| href.to_string())
            }
        });

    Ok(LinkMetadata {
        url: url.to_string(),
        title,
        description,
        image,
        favicon,
    })
} 