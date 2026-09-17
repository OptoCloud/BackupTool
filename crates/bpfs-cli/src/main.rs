use anyhow::Result;
use clap::{Parser, Subcommand};

mod cmd;

#[derive(Parser)]
#[command(name = "bpfs", version, about = "BPFS archive tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Pack {
        src: String,
        out: String,
        #[arg(long)]
        final_hash: bool,
        #[arg(long)]
        no_compress: bool,
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
            no_compress,
        } => cmd::pack::run(&src, &out, final_hash, no_compress),
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
