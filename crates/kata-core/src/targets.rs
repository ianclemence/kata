//! Where a design is going: sizes and the margins the platform's own UI covers.

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Target {
    pub id: &'static str,
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    /// Safe margins: top, right, bottom, left, in pixels.
    pub safe: [u32; 4],
}

pub const TARGETS: &[Target] = &[
    Target {
        id: "ig-square",
        name: "Instagram post, square",
        width: 1080,
        height: 1080,
        safe: [40, 40, 40, 40],
    },
    Target {
        id: "ig-portrait",
        name: "Instagram post, 4:5",
        width: 1080,
        height: 1350,
        safe: [40, 40, 40, 40],
    },
    Target {
        id: "ig-story",
        name: "Instagram / WhatsApp story, 9:16",
        width: 1080,
        height: 1920,
        safe: [250, 60, 340, 60],
    },
    Target {
        id: "tiktok",
        name: "TikTok cover, 9:16",
        width: 1080,
        height: 1920,
        safe: [160, 120, 480, 60],
    },
    Target {
        id: "x-post",
        name: "X post, 16:9",
        width: 1600,
        height: 900,
        safe: [30, 30, 30, 30],
    },
    Target {
        id: "og-image",
        name: "Link preview (Open Graph)",
        width: 1200,
        height: 630,
        safe: [30, 30, 30, 30],
    },
    Target {
        id: "a4-300dpi",
        name: "A4 print at 300 dpi",
        width: 2480,
        height: 3508,
        safe: [118, 118, 118, 118],
    },
];

pub fn by_id(id: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_unique_and_safe_areas_fit() {
        for (i, t) in super::TARGETS.iter().enumerate() {
            assert!(super::TARGETS[i + 1..].iter().all(|o| o.id != t.id));
            assert!(t.safe[1] + t.safe[3] < t.width && t.safe[0] + t.safe[2] < t.height);
        }
    }
}
