use super::CompressionPolicy;

pub struct DefaultCompressionPolicy {
    pub incompressible_entropy: f64,
}
impl CompressionPolicy for DefaultCompressionPolicy {
    fn should_compress(&self, ext: &str, h: f64) -> bool {
        const NEVER: &[&str] = &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "ico", "mp3", "ogg", "flac", "m4a", "aac",
            "mp4", "mkv", "mov", "avi", "webm", "zip", "gz", "bz2", "xz", "7z", "rar", "lz4",
            "zst", "pdf", "swf", "psd",
        ];
        if NEVER.contains(&ext) {
            return false;
        }
        h < self.incompressible_entropy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_uncompressible_extension_is_never_compressed() {
        let p = DefaultCompressionPolicy {
            incompressible_entropy: 100.0,
        };
        assert!(!p.should_compress("jpg", 0.0));
        assert!(!p.should_compress("zip", 0.0));
        assert!(!p.should_compress("mp4", 0.0));
    }

    #[test]
    fn low_entropy_text_is_compressed() {
        let p = DefaultCompressionPolicy {
            incompressible_entropy: 96.25,
        };
        assert!(p.should_compress("txt", 40.0));
    }

    #[test]
    fn high_entropy_unknown_extension_is_not_compressed() {
        let p = DefaultCompressionPolicy {
            incompressible_entropy: 96.25,
        };
        assert!(!p.should_compress("bin", 99.9));
    }

    #[test]
    fn threshold_is_exclusive() {
        let p = DefaultCompressionPolicy {
            incompressible_entropy: 50.0,
        };
        assert!(!p.should_compress("bin", 50.0));
        assert!(p.should_compress("bin", 49.999));
    }

    #[test]
    fn negative_infinity_threshold_disables_all_compression() {
        let p = DefaultCompressionPolicy {
            incompressible_entropy: f64::NEG_INFINITY,
        };
        assert!(!p.should_compress("txt", 0.0));
    }
}
