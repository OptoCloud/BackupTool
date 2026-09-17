pub mod compression;

pub trait CompressionPolicy: Send + Sync {
    fn should_compress(&self, path_ext: &str, entropy_pct: f64) -> bool;
}
