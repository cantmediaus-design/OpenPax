//! Image copy and paste through the system clipboard, so pixels can move between OpenPax and
//! other apps.

use std::borrow::Cow;

use openpax_paint::Bitmap;

/// Puts an image on the system clipboard.
pub fn copy_image(image: &Bitmap) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: Cow::Borrowed(image.data()),
        })
        .map_err(|e| e.to_string())
}

/// The image on the system clipboard, if there is one.
pub fn paste_image() -> Result<Bitmap, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let image = clipboard.get_image().map_err(|e| match e {
        arboard::Error::ContentNotAvailable => "The clipboard has no image to paste.".to_owned(),
        e => e.to_string(),
    })?;
    Bitmap::from_rgba(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or_else(|| "The clipboard image has an unexpected format.".to_owned())
}
