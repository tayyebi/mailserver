/*
 * Pixel Image Data
 * 
 * This module contains the binary data for the tracking pixel.
 * It defines a constant byte array representing a 1x1 transparent GIF image.
 * This data is served efficiently to clients requesting the tracking URL.
 */

/// 1x1 transparent GIF image data
/// This is a minimal GIF89a format image that is 1 pixel wide, 1 pixel tall, and transparent
pub const PIXEL_GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // GIF89a header
    0x01, 0x00, 0x01, 0x00,             // Width: 1, Height: 1
    0x80, 0x00, 0x00,                   // Global color table flag, color resolution, sort flag, global color table size
    0x00, 0x00, 0x00,                   // Background color index, pixel aspect ratio
    0xff, 0xff, 0xff,                   // Global color table (white)
    0x21, 0xf9, 0x04,                   // Graphic control extension
    0x01, 0x00, 0x00, 0x00, 0x00,       // Disposal method, user input flag, transparent color flag, delay time, transparent color index
    0x2c, 0x00, 0x00, 0x00, 0x00,       // Image descriptor
    0x01, 0x00, 0x01, 0x00, 0x00,       // Left, top, width, height, local color table flag
    0x02, 0x02, 0x04, 0x01, 0x00,       // LZW minimum code size, image data
    0x3b,                               // Trailer
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_gif_format() {
        // Verify the GIF header
        assert_eq!(&PIXEL_GIF[0..6], b"GIF89a");
        
        // Verify dimensions (1x1)
        let width = u16::from_le_bytes([PIXEL_GIF[6], PIXEL_GIF[7]]);
        let height = u16::from_le_bytes([PIXEL_GIF[8], PIXEL_GIF[9]]);
        assert_eq!(width, 1);
        assert_eq!(height, 1);
        
        // Verify trailer
        assert_eq!(PIXEL_GIF[PIXEL_GIF.len() - 1], 0x3b);
    }

    #[test]
    fn test_pixel_gif_size() {
        // The GIF should be small
        assert!(PIXEL_GIF.len() < 100);
        assert!(PIXEL_GIF.len() > 20);
    }
}
