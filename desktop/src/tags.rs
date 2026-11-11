//! ID3 tagging, so downloaded files carry the Spotify metadata and artwork.

use std::path::Path;

use id3::frame::{Picture, PictureType};
use id3::{Tag, TagLike, Version};

pub fn tag_mp3(
    path: &Path,
    title: &str,
    artist: &str,
    album: &str,
    cover: Option<&Path>,
) -> Result<(), String> {
    let mut tag = Tag::read_from_path(path).unwrap_or_else(|_| Tag::new());

    tag.set_title(title);
    if !artist.is_empty() {
        tag.set_artist(artist);
    }
    if !album.is_empty() {
        tag.set_album(album);
    }

    if let Some(cover) = cover {
        if let Ok(data) = std::fs::read(cover) {
            let mime_type = if data.starts_with(&[0xFF, 0xD8]) {
                "image/jpeg"
            } else {
                "image/png"
            };
            tag.add_frame(Picture {
                mime_type: mime_type.to_string(),
                picture_type: PictureType::CoverFront,
                description: "Cover".to_string(),
                data,
            });
        }
    }

    // ID3v2.3 is what Windows Explorer and older players actually display;
    // v2.4 artwork is often ignored there, which looked like "no cover".
    tag.write_to_path(path, Version::Id3v23)
        .map_err(|err| format!("não consegui gravar as tags: {err}"))
}
