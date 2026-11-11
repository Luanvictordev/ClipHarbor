//! Spotify metadata through the public embed page.
//!
//! Spotify serves no downloadable audio. The embed page carries the same
//! information the web player uses (name, artists, duration, cover artwork and
//! the track list of a playlist or album), so nothing else is needed here.

use serde_json::Value;

const EMBED_PAGE: &str = "https://open.spotify.com/embed/{kind}/{id}";

#[derive(Clone, Debug, Default)]
pub struct Track {
    /// Display title used for the file name and for the audio search.
    pub title: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<u64>,
    pub cover: String,
}

#[derive(Clone, Debug, Default)]
pub struct Entity {
    pub name: String,
    pub tracks: Vec<Track>,
}

pub fn is_spotify_url(url: &str) -> bool {
    match host_of(url) {
        Some(host) => host == "spotify.com" || host.ends_with(".spotify.com"),
        None => false,
    }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split('/').next()?;
    let host = host.rsplit('@').next().unwrap_or(host);
    Some(host.split(':').next().unwrap_or(host).to_lowercase())
}

/// Split a Spotify link into its kind ("track", "album", "playlist") and id.
pub fn link_ref(url: &str) -> Result<(String, String), String> {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let path = rest.splitn(2, '/').nth(1).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();

    for (index, part) in parts.iter().enumerate() {
        if !matches!(*part, "track" | "album" | "playlist" | "episode") {
            continue;
        }
        if let Some(id) = parts.get(index + 1) {
            let clean = id.split(['?', '#']).next().unwrap_or(id);
            if clean.len() >= 10 && clean.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Ok((part.to_string(), clean.to_string()));
            }
        }
    }

    Err("Link do Spotify não reconhecido. Use o link de uma faixa, álbum ou playlist.".into())
}

pub fn fetch(url: &str) -> Result<Entity, String> {
    let (kind, id) = link_ref(url)?;
    let page = crate::deps::curl_text(&EMBED_PAGE.replace("{kind}", &kind).replace("{id}", &id))?;
    let payload = next_data(&page)
        .ok_or_else(|| "O Spotify não devolveu os metadados deste link.".to_string())?;

    let entity = payload
        .pointer("/props/pageProps/state/data/entity")
        .ok_or_else(|| "Não consegui ler os metadados do Spotify para este link.".to_string())?;

    let cover = cover_url(entity);
    let name = text(entity, "name")
        .or_else(|| text(entity, "title"))
        .unwrap_or_else(|| "Spotify".to_string());

    let collection = kind != "track" && kind != "episode";
    // A single track's embed only knows the release it appears on, not the
    // album, so it is left out instead of writing a wrong tag.
    let album = if collection { name.as_str() } else { "" };

    let mut tracks: Vec<Track> = Vec::new();
    if let Some(list) = entity.get("trackList").and_then(Value::as_array) {
        for item in list {
            tracks.push(track_info(item, &cover, album));
        }
    }

    if tracks.is_empty() {
        tracks.push(track_info(entity, &cover, album));
    }

    Ok(Entity { name, tracks })
}

fn next_data(page: &str) -> Option<Value> {
    let marker = "id=\"__NEXT_DATA__\"";
    let start = page.find(marker)?;
    let open = page[start..].find('>')? + start + 1;
    let end = page[open..].find("</script>")? + open;
    serde_json::from_str(page[open..end].trim()).ok()
}

fn text(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn cover_url(entity: &Value) -> String {
    let mut best: (u64, String) = (0, String::new());
    let sources = entity
        .pointer("/coverArt/sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            entity
                .pointer("/visualIdentity/image")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );

    for source in sources {
        let url = source.get("url").and_then(Value::as_str).unwrap_or("");
        if url.is_empty() {
            continue;
        }
        let width = source
            .get("width")
            .or_else(|| source.get("maxWidth"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if best.1.is_empty() || width > best.0 {
            best = (width, url.to_string());
        }
    }
    best.1
}

fn track_info(track: &Value, fallback_cover: &str, fallback_album: &str) -> Track {
    let name = text(track, "title")
        .or_else(|| text(track, "name"))
        .unwrap_or_else(|| "Spotify Track".to_string());

    let artist = text(track, "subtitle").unwrap_or_else(|| {
        track
            .get("artists")
            .and_then(Value::as_array)
            .map(|artists| {
                artists
                    .iter()
                    .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    });

    let duration = track
        .get("duration")
        .and_then(Value::as_u64)
        .filter(|ms| *ms > 0)
        .map(|ms| (ms + 500) / 1000);

    let album = text(track, "album")
        .or_else(|| {
            track
                .pointer("/albumOfTrack/name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| fallback_album.to_string());

    let title = if artist.is_empty() {
        name.clone()
    } else {
        format!("{artist} - {name}")
    };

    Track {
        title,
        name,
        artist,
        album,
        duration,
        cover: fallback_cover.to_string(),
    }
}
