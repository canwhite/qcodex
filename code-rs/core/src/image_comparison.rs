use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use std::path::Path;

const HASH_WIDTH: u32 = 16;
const HASH_BITS: usize = (HASH_WIDTH * HASH_WIDTH) as usize;

fn pack_bits<I>(bits: I) -> Vec<u8>
where
    I: IntoIterator<Item = bool>,
{
    let mut hash = vec![0; HASH_BITS / 8];
    for (index, bit) in bits.into_iter().enumerate() {
        if bit {
            hash[index / 8] |= 1 << (7 - index % 8);
        }
    }
    hash
}

fn phash_256(img: &DynamicImage) -> Vec<u8> {
    let pixels = img
        .resize_exact(HASH_WIDTH, HASH_WIDTH, FilterType::Lanczos3)
        .to_luma8();
    let mean = pixels.pixels().map(|pixel| u32::from(pixel[0])).sum::<u32>()
        / HASH_BITS as u32;
    pack_bits(pixels.pixels().map(|pixel| u32::from(pixel[0]) >= mean))
}

fn dhash_256(img: &DynamicImage) -> Vec<u8> {
    let pixels = img
        .resize_exact(HASH_WIDTH + 1, HASH_WIDTH, FilterType::Lanczos3)
        .to_luma8();
    let mut bits = Vec::with_capacity(HASH_BITS);
    for y in 0..HASH_WIDTH {
        for x in 0..HASH_WIDTH {
            bits.push(pixels.get_pixel(x, y)[0] > pixels.get_pixel(x + 1, y)[0]);
        }
    }
    pack_bits(bits)
}

/// Compute a hash for an image that can be stored and compared later
pub fn compute_image_hash<P: AsRef<Path>>(path: P) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let img = ImageReader::open(path)?.decode()?;
    let phash = phash_256(&img);
    let dhash = dhash_256(&img);

    Ok((phash, dhash))
}

/// Compare image hashes to determine if images are similar
pub fn are_hashes_similar(phash1: &[u8], dhash1: &[u8], phash2: &[u8], dhash2: &[u8]) -> bool {
    if phash1.len() != 32 || dhash1.len() != 32 || phash2.len() != 32 || dhash2.len() != 32 {
        return false;
    }

    // Count differing bits (Hamming distance)
    let phash_dist = phash1
        .iter()
        .zip(phash2.iter())
        .map(|(a, b)| (a ^ b).count_ones() as u32)
        .sum::<u32>();

    let dhash_dist = dhash1
        .iter()
        .zip(dhash2.iter())
        .map(|(a, b)| (a ^ b).count_ones() as u32)
        .sum::<u32>();

    // 256 bits → ~5% tolerance (≈13 bits)
    phash_dist <= 13 && dhash_dist <= 13
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    #[test]
    fn computed_hashes_are_256_bits_and_match_the_same_image() {
        let image = DynamicImage::ImageLuma8(GrayImage::from_fn(32, 32, |x, y| {
            Luma([u8::try_from(x + y).unwrap()])
        }));

        let phash = phash_256(&image);
        let dhash = dhash_256(&image);

        assert_eq!(phash.len(), 32);
        assert_eq!(dhash.len(), 32);
        assert!(are_hashes_similar(&phash, &dhash, &phash, &dhash));
    }
}
