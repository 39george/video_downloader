use std::path::PathBuf;

use clap::Parser;
use tracing_subscriber::{filter::FilterExt, layer::SubscriberExt};

pub mod pingora_proxy;
pub mod proxy;
mod run;
pub mod video_saver;

/// This macro is for tracing error and returning Result if there are some
/// meaningful Ok() case, and returning () if there are no meaningful result.
/// It is useful to simply trace error message on fallible operations which doesn't
/// return anything in the Ok() branch.
#[macro_export]
macro_rules! print_err {
    ($exp:expr) => {
        match $exp {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::error!("{e}");
                Err(e)
            }
        }
    };
    ($exp:expr, ()) => {
        match $exp {
            Ok(()) => (),
            Err(e) => {
                tracing::error!("{e}");
                ()
            }
        }
    };
}

/// Robot to download medical videos
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Authentication email
    #[arg(short, long)]
    email: String,

    /// Authentication password
    #[arg(short, long)]
    password: String,

    #[arg(short, long, default_value_t = String::from("http://localhost:4444"))]
    geckodriver_address: String,

    #[arg(short, long, default_value_t = 8080)]
    proxy_port: u16,

    #[arg(short, long, default_value_t = String::from("/teach/control"))]
    base: String,
    #[arg(short, long, default_value_t = String::from("/teach/control/stream/index"))]
    root: String,
    #[arg(short, long, default_value_t = String::from("https://universkill.ru"))]
    domain: String,
    #[arg(short, long, default_value_t = String::from("https://universkill.ru/cms/system/login?required=true"))]
    auth_url: String,

    /// If you need to download only specified videos
    #[arg(short, long)]
    path_to_videos_info_file: Option<PathBuf>,
}

fn main() {
    init_tracing_subscriber();
    tracing::info!("Hello world!");
    let args = Args::parse();
    run::run(args).unwrap();
}

fn init_tracing_subscriber() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .with_level(true)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("hudsucker=off".parse().unwrap())
                .add_directive("video_downloader=info".parse().unwrap()),
        )
        .finish()
        .with(
            tracing_subscriber::fmt::layer().with_writer(
                std::fs::File::options()
                    .create(true)
                    .append(true)
                    .write(true)
                    .open("downloader.log")
                    .expect("Can't open log file!"),
            ),
        );
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set up tracing");
}
