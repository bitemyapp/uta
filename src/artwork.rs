//! Bounded, URL-keyed thumbnail scheduling. Fetching and decoding run in workers.
use aizome_kit::prelude::Image;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    time::Duration,
};

const CONCURRENT: usize = 4;
const CACHE_BYTES: u64 = 32 * 1024 * 1024;
const CACHE_ENTRIES: usize = 80;

#[derive(Default)]
pub struct Thumbnails {
    images: Rc<HashMap<String, Image>>,
    order: VecDeque<String>,
    wanted: HashSet<String>,
    pending: VecDeque<String>,
    active: HashSet<String>,
    bytes: u64,
}

impl Thumbnails {
    pub fn images(&self) -> Rc<HashMap<String, Image>> {
        self.images.clone()
    }

    /// Replaces queued work when results change. Already running jobs retain
    /// their slots until completion, so rapid searches cannot create a burst.
    pub fn plan(&mut self, urls: impl IntoIterator<Item = String>) {
        self.wanted.clear();
        self.pending.clear();
        for url in urls {
            if safe_url(&url)
                && self.wanted.insert(url.clone())
                && !self.images.contains_key(&url)
                && !self.active.contains(&url)
            {
                self.pending.push_back(url);
            }
        }
    }

    pub fn next_request(&mut self) -> Option<String> {
        if self.active.len() >= CONCURRENT {
            return None;
        }
        let url = self.pending.pop_front()?;
        self.active.insert(url.clone());
        Some(url)
    }

    pub fn finish(&mut self, url: String, image: Option<Image>) {
        self.active.remove(&url);
        // A late result belongs to its URL, never to a row index in a new search.
        if !self.wanted.contains(&url) || self.images.contains_key(&url) {
            return;
        }
        let Some(image) = image else {
            return;
        };
        let cost = image_bytes(&image);
        if cost > CACHE_BYTES {
            return;
        }
        while self.bytes + cost > CACHE_BYTES || self.order.len() >= CACHE_ENTRIES {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(image) = Rc::make_mut(&mut self.images).remove(&old) {
                self.bytes -= image_bytes(&image);
            }
        }
        self.bytes += cost;
        self.order.push_back(url.clone());
        Rc::make_mut(&mut self.images).insert(url, image);
    }

    pub fn clear(&mut self) {
        self.plan([]);
        self.images = Rc::default();
        self.order.clear();
        self.bytes = 0;
    }
}

fn image_bytes(image: &Image) -> u64 {
    let (width, height) = image.pixel_size();
    u64::from(width) * u64::from(height) * 4
}

pub fn safe_url(url: &str) -> bool {
    let Some(host) = url
        .strip_prefix("https://")
        .and_then(|u| u.split('/').next())
    else {
        return false;
    };
    !host.contains(['@', ':'])
        && ["ytimg.com", "googleusercontent.com", "ggpht.com"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// No account headers, redirects, disk cache, or unbounded response/decoded size.
pub fn fetch(url: &str) -> Option<Image> {
    if !safe_url(url) {
        return None;
    }
    let bytes = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .max_redirects(0)
        .build()
        .new_agent()
        .get(url)
        .call()
        .ok()?
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_vec()
        .ok()?;
    decode(bytes)
}

fn decode(bytes: Vec<u8>) -> Option<Image> {
    let image = Image::decode(bytes)?;
    let (width, height) = image.pixel_size();
    if u64::from(width) * u64::from(height) > 4 * 1024 * 1024 {
        return None;
    }
    // Image::decode reads only headers. Materialize the pixels here, off the UI
    // and render threads, so scrolling never needs to decode a fetched image.
    Image::from_rgba(width, height, &image.to_rgba8()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url(i: usize) -> String {
        format!("https://i.ytimg.com/{i}.jpg")
    }
    fn pixel() -> Image {
        Image::from_rgba(1, 1, &[255; 4]).unwrap()
    }

    #[test]
    fn requests_are_bounded_deduplicated_and_replaced_on_navigation() {
        let mut cache = Thumbnails::default();
        cache.plan((0..10).map(url).chain([url(0)]));
        for i in 0..4 {
            assert_eq!(cache.next_request(), Some(url(i)));
        }
        assert!(cache.next_request().is_none());
        cache.plan([url(20)]);
        assert!(cache.next_request().is_none());
        cache.finish(url(0), Some(pixel()));
        assert!(cache.images().is_empty());
        assert_eq!(cache.next_request(), Some(url(20)));
        assert!(cache.next_request().is_none());
    }

    #[test]
    fn cache_reuse_and_failures_do_not_retry_forever() {
        let mut cache = Thumbnails::default();
        cache.plan([url(1), url(2)]);
        let first = cache.next_request().unwrap();
        cache.finish(first, Some(pixel()));
        let second = cache.next_request().unwrap();
        cache.finish(second, None);
        assert!(cache.next_request().is_none());
        cache.plan([url(1), url(2)]);
        assert_eq!(cache.next_request(), Some(url(2)));
        assert!(cache.next_request().is_none());
    }

    #[test]
    fn retained_view_snapshots_survive_bounded_cache_eviction() {
        let mut cache = Thumbnails::default();
        cache.plan([url(0)]);
        cache.finish(url(0), Some(pixel()));
        let snapshot = cache.images();
        for i in 1..=CACHE_ENTRIES {
            cache.plan([url(i)]);
            cache.finish(url(i), Some(pixel()));
        }
        assert_eq!(cache.images().len(), CACHE_ENTRIES);
        assert!(!cache.images().contains_key(&url(0)));
        assert!(snapshot.contains_key(&url(0)));
    }

    #[test]
    fn invalid_images_and_non_youtube_urls_are_ignored() {
        assert!(decode(b"not an image".to_vec()).is_none());
        for url in [
            "http://i.ytimg.com/a.jpg",
            "https://ytimg.com.evil.test/a",
            "https://evil@i.ytimg.com/a",
            "file:///etc/passwd",
        ] {
            assert!(!safe_url(url));
        }
        assert!(safe_url("https://lh3.googleusercontent.com/a"));
    }
}
