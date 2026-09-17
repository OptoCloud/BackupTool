use anyhow::Result;
use bpfs_core::types::enums::CompressionType;
use clap::{Parser, Subcommand, ValueEnum};

mod cmd;

#[derive(Parser)]
#[command(name = "bpfs", version, about = "BPFS archive tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Codec {
    Zstd,
    Brotli,
    None,
}

impl From<Codec> for CompressionType {
    fn from(c: Codec) -> Self {
        match c {
            Codec::Zstd => CompressionType::Zstd,
            Codec::Brotli => CompressionType::Brotli,
            Codec::None => CompressionType::None,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    Pack {
        src: String,
        out: String,
        #[arg(long)]
        final_hash: bool,
        /// Compression for compressible files
        #[arg(long, value_enum, default_value_t = Codec::Zstd)]
        compression: Codec,
    },
    Ls {
        archive: String,
        #[arg(long)]
        tree: bool,
    },
    Extract {
        archive: String,
        dest: String,
        #[arg(long, value_name="PATH", num_args=1..)]
        path: Vec<String>,
        #[arg(long)]
        overwrite: bool,
    },
    Verify {
        archive: String,
        #[arg(long)]
        deep: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Pack {
            src,
            out,
            final_hash,
            compression,
        } => cmd::pack::run(&src, &out, final_hash, compression.into()),
        Command::Ls { archive, tree } => cmd::ls::run(&archive, tree),
        Command::Extract {
            archive,
            dest,
            path,
            overwrite,
        } => cmd::extract::run(&archive, &dest, &path, overwrite),
        Command::Verify { archive, deep } => cmd::verify::run(&archive, deep),
    }
}
