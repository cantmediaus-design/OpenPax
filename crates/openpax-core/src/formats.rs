/// Resource pack format numbers by Minecraft: Java Edition release, newest first.
///
/// The table is a convenience for the "New Pack" wizard; any number can still be typed by hand.
pub const RESOURCE_FORMATS: &[(&str, u32)] = &[
    ("1.21.11", 75),
    ("1.21.9 – 1.21.10", 69),
    ("1.21.7 – 1.21.8", 64),
    ("1.21.6", 63),
    ("1.21.5", 55),
    ("1.21.4", 46),
    ("1.21.2 – 1.21.3", 42),
    ("1.21 – 1.21.1", 34),
    ("1.20.5 – 1.20.6", 32),
    ("1.20.3 – 1.20.4", 22),
    ("1.20.2", 18),
    ("1.20 – 1.20.1", 15),
    ("1.19.4", 13),
    ("1.19.3", 12),
    ("1.19 – 1.19.2", 9),
    ("1.18 – 1.18.2", 8),
    ("1.17 – 1.17.1", 7),
    ("1.16.2 – 1.16.5", 6),
];

/// The release label for a known format number.
pub fn version_for_format(format: u32) -> Option<&'static str> {
    RESOURCE_FORMATS
        .iter()
        .find(|(_, f)| *f == format)
        .map(|(v, _)| *v)
}
