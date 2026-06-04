//! Feeds data model and providers for the Welcome screen.
//!
//! The Feeds tab shows announcements, release notes, and project updates.
//! This module is intentionally network-free: the only provider today is
//! [`StaticFeedProvider`], which returns a curated set of offline items. A
//! future `RemoteFeedProvider` can implement [`FeedProvider`] to fetch + cache
//! JSON from the Futureboard website without changing any UI code.

/// High-level classification for a feed item, used for the category badge and
/// the Feeds filter chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedCategory {
    Announcement,
    Release,
    Development,
    Community,
    Roadmap,
}

impl FeedCategory {
    /// Short label shown on the category badge / filter chip.
    pub fn label(self) -> &'static str {
        match self {
            FeedCategory::Announcement => "Announcement",
            FeedCategory::Release => "Release",
            FeedCategory::Development => "Development",
            FeedCategory::Community => "Community",
            FeedCategory::Roadmap => "Roadmap",
        }
    }
}

/// A single entry in the Feeds tab.
#[derive(Debug, Clone)]
pub struct FeedItem {
    pub id: String,
    pub title: String,
    pub category: FeedCategory,
    /// Human-readable date string (e.g. "2026-06-04"). Kept as a string so the
    /// static provider and a future JSON provider share one shape.
    pub date: String,
    pub summary: String,
    /// Optional external link, opened in the system browser when present.
    pub url: Option<String>,
    pub unread: bool,
}

/// Source of feed items. Implementors must not block the UI thread; the static
/// provider returns instantly, and a remote provider should fetch on a
/// background task and hand results back to the UI.
pub trait FeedProvider {
    fn load_feed_items(&self) -> Vec<FeedItem>;
}

/// Offline provider with a curated set of items. Used as the default and as the
/// fallback when a remote provider is unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct StaticFeedProvider;

impl FeedProvider for StaticFeedProvider {
    fn load_feed_items(&self) -> Vec<FeedItem> {
        vec![
            FeedItem {
                id: "welcome".to_string(),
                title: "Welcome to Futureboard Studio".to_string(),
                category: FeedCategory::Announcement,
                date: "2026-06-04".to_string(),
                summary: "Futureboard Studio is in active native development.".to_string(),
                url: None,
                unread: true,
            },
            FeedItem {
                id: "native-gpui".to_string(),
                title: "Native GPUI build".to_string(),
                category: FeedCategory::Development,
                date: "2026-05-28".to_string(),
                summary: "The native GPUI shell is now the primary Studio surface.".to_string(),
                url: None,
                unread: true,
            },
            FeedItem {
                id: "marketplace".to_string(),
                title: "Marketplace and plugin systems".to_string(),
                category: FeedCategory::Roadmap,
                date: "2026-05-15".to_string(),
                summary: "Sample, preset, plugin, and audio extension systems are being designed."
                    .to_string(),
                url: None,
                unread: false,
            },
            FeedItem {
                id: "daily-builds".to_string(),
                title: "Daily builds".to_string(),
                category: FeedCategory::Release,
                date: "2026-05-02".to_string(),
                summary: "Experimental builds may change quickly and include unfinished systems."
                    .to_string(),
                url: None,
                unread: false,
            },
        ]
    }
}

/// Filter applied to the Feeds list. `All` shows everything; the rest match a
/// single [`FeedCategory`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedFilter {
    All,
    Announcements,
    Releases,
    Development,
    Community,
}

impl FeedFilter {
    pub fn label(self) -> &'static str {
        match self {
            FeedFilter::All => "All",
            FeedFilter::Announcements => "Announcements",
            FeedFilter::Releases => "Releases",
            FeedFilter::Development => "Development",
            FeedFilter::Community => "Community",
        }
    }

    /// All selectable filters, in display order.
    pub fn all() -> [FeedFilter; 5] {
        [
            FeedFilter::All,
            FeedFilter::Announcements,
            FeedFilter::Releases,
            FeedFilter::Development,
            FeedFilter::Community,
        ]
    }

    /// Whether `item` should be shown under this filter.
    pub fn matches(self, item: &FeedItem) -> bool {
        match self {
            FeedFilter::All => true,
            FeedFilter::Announcements => item.category == FeedCategory::Announcement,
            FeedFilter::Releases => item.category == FeedCategory::Release,
            FeedFilter::Development => item.category == FeedCategory::Development,
            FeedFilter::Community => item.category == FeedCategory::Community,
        }
    }
}
