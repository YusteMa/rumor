//! The ambient catalogue and its downloads (SPEC §3.5).
//!
//! Ambients are real recordings, too big to ship in the installer, so each
//! one is downloaded the first time the user picks it: from this project's
//! GitHub release `ambients-v1`, checked against the size and SHA-256 below
//! before it is ever played, and kept in the app's data folder. That is the
//! second and last network request Rumor makes (CLAUDE.md rule 8): only on
//! the user's click, and nothing about the user is sent.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;

/// Where the prepared files are published.
const BASE_URL: &str = "https://github.com/YusteMa/rumor/releases/download/ambients-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ambient {
    /// Stable id: saved in settings and in the study history.
    pub id: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    /// The recording and its author, for the credits (all CC0).
    pub credit: &'static str,
}

/// In the order the panel shows them.
pub const CATALOG: [Ambient; 4] = [
    Ambient {
        id: "rain-soft",
        file: "rain-soft.flac",
        bytes: 67_900_741,
        sha256: "c5a26ce9a74932b52a75e06c699eb817c224c94e56b3be2b58e2c81136a7fbb6",
        credit: "\"Forest cabin summer rain\" by giddster (freesound.org, CC0)",
    },
    Ambient {
        id: "rain",
        file: "rain.flac",
        bytes: 76_316_311,
        sha256: "3c182bcae8d297154170f98dbf98ef3cd430735b591d768f3699e23e23efd033",
        credit: "\"rain in the forest atmo field recording\" by Garuda1982 (freesound.org, CC0)",
    },
    Ambient {
        id: "storm",
        file: "storm.flac",
        bytes: 29_946_020,
        sha256: "0ef23a07dd9ce25b888de6b780bbe12028a4e0c8672fb08b361ae95216184e1b",
        credit: "\"Excellent thunder & rain 25 Sept '21\" by gepetto3548 (freesound.org, CC0)",
    },
    Ambient {
        id: "downpour",
        file: "downpour.flac",
        bytes: 79_656_131,
        sha256: "6ad443221e872d3d6f7db8bea1ea7ca95a4cb9374b6f158132ec5a60f7ac6868",
        credit: "\"Rain and thunder in Thailand\" by felix.blume (freesound.org, CC0)",
    },
];

pub fn find(id: &str) -> Option<&'static Ambient> {
    CATALOG.iter().find(|a| a.id == id)
}

/// Where a ready ambient lives.
pub fn path(dir: &Path, a: &Ambient) -> PathBuf {
    dir.join(a.file)
}

/// The file is there and has the expected size. Its hash was checked when it
/// was downloaded; the size catches a file truncated since.
pub fn is_ready(dir: &Path, a: &Ambient) -> bool {
    a.bytes > 0 && std::fs::metadata(path(dir, a)).is_ok_and(|m| m.len() == a.bytes)
}

/// One ambient as the panel shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbientView {
    pub id: &'static str,
    pub status: Status,
    /// 0..=1 while downloading.
    pub progress: f32,
    /// Download size, for the panel's hint.
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Not downloaded yet: the first click downloads it.
    Missing,
    Downloading,
    Ready,
    /// The last download failed; the next click tries again.
    Failed,
    /// Not published yet (no size and hash in the catalogue).
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("the server answered {0}")]
    Status(reqwest::StatusCode),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the download is {got} bytes, expected {expected}")]
    Size { got: u64, expected: u64 },
    #[error("the download is damaged (SHA-256 mismatch)")]
    Hash,
    #[error("this ambient is not published yet")]
    Unavailable,
}

fn client() -> Result<reqwest::Client, reqwest::Error> {
    // Same TLS set-up as the updater: rustls with the ring provider.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::Client::builder()
        .user_agent(concat!("Rumor/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Downloads `a` into `dir`, calling `progress(done, total)` as it goes.
/// Writes to a `.part` file and renames it only once size and hash match,
/// so a half-downloaded or damaged file is never played.
pub async fn download(
    dir: &Path,
    a: &Ambient,
    progress: impl Fn(u64, u64),
) -> Result<PathBuf, DownloadError> {
    if a.bytes == 0 || a.sha256.is_empty() {
        return Err(DownloadError::Unavailable);
    }
    // Plain file calls: a few hundred kilobytes per chunk, on Tauri's pool.
    std::fs::create_dir_all(dir)?;
    let part = dir.join(format!("{}.part", a.file));
    let result = fetch(&part, a, progress).await;
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result?;
    let target = path(dir, a);
    std::fs::rename(&part, &target)?;
    Ok(target)
}

async fn fetch(part: &Path, a: &Ambient, progress: impl Fn(u64, u64)) -> Result<(), DownloadError> {
    let url = format!("{BASE_URL}/{}", a.file);
    let mut response = client()?.get(url).send().await?;
    if !response.status().is_success() {
        return Err(DownloadError::Status(response.status()));
    }
    let mut file = std::io::BufWriter::new(std::fs::File::create(part)?);
    let mut hash = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = response.chunk().await? {
        done += chunk.len() as u64;
        if done > a.bytes {
            return Err(DownloadError::Size {
                got: done,
                expected: a.bytes,
            });
        }
        hash.update(&chunk);
        file.write_all(&chunk)?;
        progress(done, a.bytes);
    }
    file.flush()?;
    drop(file);
    if done != a.bytes {
        return Err(DownloadError::Size {
            got: done,
            expected: a.bytes,
        });
    }
    if !hex(&hash.finalize()).eq_ignore_ascii_case(a.sha256) {
        return Err(DownloadError::Hash);
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_files_are_unique() {
        for (i, a) in CATALOG.iter().enumerate() {
            for b in &CATALOG[i + 1..] {
                assert_ne!(a.id, b.id);
                assert_ne!(a.file, b.file);
            }
            // Never a colour name: the study history stores both in one column.
            assert!(!["pink", "brown", "white"].contains(&a.id));
            assert_eq!(find(a.id), Some(a));
        }
    }

    #[test]
    fn published_entries_have_a_full_hash() {
        for a in CATALOG.iter().filter(|a| a.bytes > 0) {
            assert_eq!(a.sha256.len(), 64, "{}", a.id);
            assert!(a.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn hex_matches_a_known_digest() {
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
