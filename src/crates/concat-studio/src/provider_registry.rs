// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kedi Video Factory – Provider Registry.
//!
//! Declares every platform's real capabilities without fabricating support.
//! Each `ProviderDef` is a *declaration*, not an implementation.
//! The download adapters (`lux`, `yt-dlp`) are queried at runtime;
//! this registry only describes what the code can plausibly attempt
//! and what requires manual URL entry.
//!
//! # Capability tiers
//! - `supported_in_code`  – logic exists and compiles.
//! - `configured`         – binary/API present on this machine.
//! - `verified_locally`   – passed a smoke test with a real URL.
//! - `verified_in_staging`– confirmed in a controlled environment.
//!
//! Capabilities that are `false` in `supported_in_code` must never be
//! exercised by the UI; the registry is the single source of truth.

use serde::{Deserialize, Serialize};

/// A single provider's honest capability profile.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderDef {
    /// Stable machine ID (e.g. `"youtube"`, `"tiktok"`).
    pub id: &'static str,
    /// Display name shown in the UI.
    pub display_name: &'static str,
    /// URL patterns that identify this provider (prefix/host match).
    pub url_patterns: &'static [&'static str],
    /// Provider category for grouping in the UI.
    pub category: ProviderCategory,
    /// Underlying downloader backend used when downloading.
    pub backend: DownloadBackend,

    // ── Video capabilities ──────────────────────────────────────────────────
    pub resolve_metadata: bool,
    pub download_video: bool,
    pub download_audio: bool,
    pub playlist: bool,
    pub search: bool,

    // ── Product / e-commerce capabilities ──────────────────────────────────
    pub product_metadata: bool,

    // ── Publishing capabilities ─────────────────────────────────────────────
    pub publish: bool,

    // ── Authentication ──────────────────────────────────────────────────────
    /// Whether this provider requires authentication to use any capability.
    pub requires_auth: bool,
    /// Whether this provider is currently available (not region-blocked, etc.).
    pub availability: ProviderAvailability,

    // ── Verification tier ───────────────────────────────────────────────────
    pub supported_in_code: bool,
    pub configured: bool,
    pub verified_locally: bool,
    pub verified_in_staging: bool,

    /// ISO-8601 date of last successful local test, or `None`.
    pub verified_at: Option<&'static str>,
    /// Human-readable reason when unavailable.
    pub unavailable_reason: Option<&'static str>,
}

/// Which downloader backend handles this provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadBackend {
    /// yt-dlp – verified primary for YouTube, TikTok, many others.
    YtDlp,
    /// lux – Go-based, handles Bilibili, some CN platforms.
    Lux,
    /// Manual URL / file input only; no automated extraction.
    ManualOnly,
    /// Custom adapter (product scraping, structured-data parsing, etc.).
    Custom,
}

/// Broad category for grouping in the provider settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderCategory {
    VideoSharing,
    ShortFormVideo,
    ECommerce,
    LiveStreaming,
    Social,
    General,
}

/// Runtime availability after capability check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderAvailability {
    /// Binary/API present and reachable.
    Available,
    /// Binary missing (yt-dlp or lux not found).
    BinaryMissing,
    /// Requires explicit user configuration.
    NeedsConfig,
    /// Not implemented yet.
    NotImplemented,
    /// Platform restriction or unknown.
    Unknown,
}

impl ProviderDef {
    /// True when the provider can actually be used for download right now.
    pub fn can_download(&self) -> bool {
        self.supported_in_code && self.download_video && self.configured
    }

    /// True when the user should be shown a manual URL entry fallback.
    pub fn needs_manual_fallback(&self) -> bool {
        !self.can_download()
            || matches!(self.availability, ProviderAvailability::BinaryMissing | ProviderAvailability::NotImplemented)
    }

    /// Short status string for the UI badge.
    pub fn status_label(&self) -> &'static str {
        if !self.supported_in_code {
            return "Chưa hỗ trợ";
        }
        match self.availability {
            ProviderAvailability::Available => {
                if self.verified_locally { "Đã xác minh" } else { "Có thể dùng" }
            }
            ProviderAvailability::BinaryMissing => "Thiếu công cụ",
            ProviderAvailability::NeedsConfig => "Cần cấu hình",
            ProviderAvailability::NotImplemented => "Chưa hỗ trợ",
            ProviderAvailability::Unknown => "Chưa kiểm tra",
        }
    }
}

/// The global registry – one entry per platform. All fields are honest.
/// `configured` and `availability` are updated at runtime by `check_runtime_availability`.
pub static PROVIDERS: &[ProviderDef] = &[
    // ── YouTube / Shorts ────────────────────────────────────────────────────
    ProviderDef {
        id: "youtube",
        display_name: "YouTube / Shorts",
        url_patterns: &["youtube.com", "youtu.be"],
        category: ProviderCategory::VideoSharing,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: true,
        playlist: true,
        search: false, // yt-dlp search is unstable; not wired
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── TikTok ─────────────────────────────────────────────────────────────
    ProviderDef {
        id: "tiktok",
        display_name: "TikTok",
        url_patterns: &["tiktok.com", "www.tiktok.com"],
        category: ProviderCategory::ShortFormVideo,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: true, // via browser fallback (download_browser.rs)
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Douyin ─────────────────────────────────────────────────────────────
    ProviderDef {
        id: "douyin",
        display_name: "Douyin",
        url_patterns: &["douyin.com"],
        category: ProviderCategory::ShortFormVideo,
        backend: DownloadBackend::Lux,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Facebook ────────────────────────────────────────────────────────────
    ProviderDef {
        id: "facebook",
        display_name: "Facebook Video / Reels",
        url_patterns: &["facebook.com", "fb.watch", "www.facebook.com"],
        category: ProviderCategory::Social,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false, // public videos; auth needed for private
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Instagram ──────────────────────────────────────────────────────────
    ProviderDef {
        id: "instagram",
        display_name: "Instagram Video / Reels",
        url_patterns: &["instagram.com", "www.instagram.com"],
        category: ProviderCategory::Social,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: true, // most content requires cookies
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: Some("Hầu hết video Instagram yêu cầu đăng nhập. Dán URL video công khai hoặc nhập file thủ công."),
    },
    // ── X (Twitter) ─────────────────────────────────────────────────────────
    ProviderDef {
        id: "x_twitter",
        display_name: "X (Twitter)",
        url_patterns: &["x.com", "twitter.com"],
        category: ProviderCategory::Social,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Reddit ──────────────────────────────────────────────────────────────
    ProviderDef {
        id: "reddit",
        display_name: "Reddit",
        url_patterns: &["reddit.com", "www.reddit.com", "v.redd.it"],
        category: ProviderCategory::Social,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Vimeo ───────────────────────────────────────────────────────────────
    ProviderDef {
        id: "vimeo",
        display_name: "Vimeo",
        url_patterns: &["vimeo.com", "www.vimeo.com"],
        category: ProviderCategory::VideoSharing,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Dailymotion ─────────────────────────────────────────────────────────
    ProviderDef {
        id: "dailymotion",
        display_name: "Dailymotion",
        url_patterns: &["dailymotion.com", "dai.ly"],
        category: ProviderCategory::VideoSharing,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Bilibili ────────────────────────────────────────────────────────────
    ProviderDef {
        id: "bilibili",
        display_name: "Bilibili",
        url_patterns: &["bilibili.com", "www.bilibili.com", "b23.tv"],
        category: ProviderCategory::VideoSharing,
        backend: DownloadBackend::Lux,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: true,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Pinterest ───────────────────────────────────────────────────────────
    ProviderDef {
        id: "pinterest",
        display_name: "Pinterest",
        url_patterns: &["pinterest.com", "pin.it"],
        category: ProviderCategory::Social,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Twitch ──────────────────────────────────────────────────────────────
    ProviderDef {
        id: "twitch",
        display_name: "Twitch Clip / VOD",
        url_patterns: &["twitch.tv", "clips.twitch.tv"],
        category: ProviderCategory::LiveStreaming,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: None,
    },
    // ── Generic public video (catch-all via yt-dlp) ─────────────────────────
    ProviderDef {
        id: "generic_web",
        display_name: "Website video công khai khác",
        url_patterns: &[],
        category: ProviderCategory::General,
        backend: DownloadBackend::YtDlp,
        resolve_metadata: true,
        download_video: true,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::Unknown,
        supported_in_code: true,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: Some("Hỗ trợ phụ thuộc vào phiên bản yt-dlp hiện có. Dán URL video cụ thể để thử."),
    },
    // ── E-commerce: Shopee ──────────────────────────────────────────────────
    ProviderDef {
        id: "shopee",
        display_name: "Shopee",
        url_patterns: &["shopee.vn", "shopee.com"],
        category: ProviderCategory::ECommerce,
        backend: DownloadBackend::Custom,
        resolve_metadata: false,
        download_video: false,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false, // structured-data scraping not yet implemented
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::NotImplemented,
        supported_in_code: false,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: Some("Phân tích sản phẩm Shopee chưa được triển khai. Dán URL sản phẩm thủ công hoặc nhập thông tin bằng tay."),
    },
    // ── E-commerce: Lazada ──────────────────────────────────────────────────
    ProviderDef {
        id: "lazada",
        display_name: "Lazada",
        url_patterns: &["lazada.vn", "lazada.com"],
        category: ProviderCategory::ECommerce,
        backend: DownloadBackend::Custom,
        resolve_metadata: false,
        download_video: false,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::NotImplemented,
        supported_in_code: false,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: Some("Phân tích sản phẩm Lazada chưa được triển khai. Nhập thông tin sản phẩm thủ công."),
    },
    // ── E-commerce: TikTok Shop ─────────────────────────────────────────────
    ProviderDef {
        id: "tiktok_shop",
        display_name: "TikTok Shop",
        url_patterns: &["tiktokshop.com"],
        category: ProviderCategory::ECommerce,
        backend: DownloadBackend::Custom,
        resolve_metadata: false,
        download_video: false,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::NotImplemented,
        supported_in_code: false,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: Some("Phân tích sản phẩm TikTok Shop chưa được triển khai. Nhập thủ công."),
    },
    // ── E-commerce: Amazon ──────────────────────────────────────────────────
    ProviderDef {
        id: "amazon",
        display_name: "Amazon",
        url_patterns: &["amazon.com", "amazon.vn"],
        category: ProviderCategory::ECommerce,
        backend: DownloadBackend::Custom,
        resolve_metadata: false,
        download_video: false,
        download_audio: false,
        playlist: false,
        search: false,
        product_metadata: false,
        publish: false,
        requires_auth: false,
        availability: ProviderAvailability::NotImplemented,
        supported_in_code: false,
        configured: false,
        verified_locally: false,
        verified_in_staging: false,
        verified_at: None,
        unavailable_reason: Some("Phân tích sản phẩm Amazon chưa được triển khai. Nhập thủ công."),
    },
];

/// Check which backends are available on this machine and update `configured`/`availability`.
/// Returns a list of provider IDs that are now usable.
pub fn check_runtime_availability(ytdlp_found: bool, lux_found: bool) -> Vec<String> {
    let mut usable = Vec::new();
    for p in PROVIDERS {
        let available = match p.backend {
            DownloadBackend::YtDlp => ytdlp_found,
            DownloadBackend::Lux => lux_found,
            DownloadBackend::ManualOnly => false,
            DownloadBackend::Custom => false,
        };
        if p.supported_in_code && available {
            usable.push(p.id.to_owned());
        }
    }
    usable
}

/// Match a URL to the best provider definition.
pub fn provider_for_url(url: &str) -> Option<&'static ProviderDef> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    // Exact host / suffix match on declared patterns
    for def in PROVIDERS {
        for pattern in def.url_patterns {
            if host == *pattern || host.ends_with(&format!(".{pattern}")) {
                return Some(def);
            }
        }
    }
    // Fall through to generic
    PROVIDERS
        .iter()
        .find(|p| p.id == "generic_web")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_providers_have_unique_ids() {
        let mut ids = std::collections::HashSet::new();
        for p in PROVIDERS {
            assert!(ids.insert(p.id), "duplicate provider id: {}", p.id);
        }
    }

    #[test]
    fn ecommerce_providers_are_not_in_code() {
        for p in PROVIDERS.iter().filter(|p| p.category == ProviderCategory::ECommerce) {
            assert!(
                !p.supported_in_code,
                "e-commerce provider {} claims supported_in_code but has no implementation",
                p.id
            );
        }
    }

    #[test]
    fn youtube_matches_url() {
        assert_eq!(
            provider_for_url("https://www.youtube.com/watch?v=abc").map(|p| p.id),
            Some("youtube")
        );
        assert_eq!(
            provider_for_url("https://youtu.be/abc").map(|p| p.id),
            Some("youtube")
        );
    }

    #[test]
    fn tiktok_matches_url() {
        assert_eq!(
            provider_for_url("https://www.tiktok.com/@user/video/123").map(|p| p.id),
            Some("tiktok")
        );
    }

    #[test]
    fn runtime_availability_requires_binary() {
        let usable = check_runtime_availability(false, false);
        assert!(usable.is_empty(), "no binary → no usable providers");
        let usable_ytdlp = check_runtime_availability(true, false);
        assert!(usable_ytdlp.contains(&"youtube".to_owned()));
        assert!(!usable_ytdlp.contains(&"bilibili".to_owned()));
        let usable_both = check_runtime_availability(true, true);
        assert!(usable_both.contains(&"bilibili".to_owned()));
    }
}
