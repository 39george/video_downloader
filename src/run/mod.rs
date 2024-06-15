use std::collections::HashSet;
use std::fs::{read_to_string, File};
use std::io::Write;
use std::process::exit;
use std::time::Duration;

use futures::future::{BoxFuture, FutureExt};
use thirtyfour::extensions::query::ElementWaitable;
use thirtyfour::{
    By, CapabilitiesHelper, Cookie, DesiredCapabilities, WebDriver,
};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

use crate::proxy::{self, Signal};
use crate::video_saver::{VideoInfo, VideoSaver};
use crate::{print_err, Args};

use self::href::Href;

mod href;

#[tokio::main]
pub async fn run(args: Args) -> Result<(), anyhow::Error> {
    // Prepare communication
    let interceptor_tx = run_proxy().await.unwrap();
    let (saver_tx, saver_rx) = tokio::sync::mpsc::channel(10000);
    let video_saver = VideoSaver::new(saver_rx);
    let video_saver_handle = video_saver.run_video_saver();

    if let Some(path) = args.path_to_videos_info_file {
        if !path.exists() {
            tracing::error!("Not found file: {}", path.to_string_lossy());
            exit(1);
        }
        let contents = match read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    "Failed to read file: {}, error: {e}",
                    path.to_string_lossy()
                );
                exit(2);
            }
        };
        let info: Vec<VideoInfo> = match serde_json::from_str(&contents) {
            Ok(i) => i,
            Err(e) => {
                tracing::error!("Failed to deserialize videos info: {e}",);
                exit(3);
            }
        };
        for i in info {
            saver_tx.send(i).await?;
        }
        // Drop tx so video saver can stop
        drop(saver_tx);
        wait_video_saver(video_saver_handle).await;
        exit(0);
    }

    // Setup webdriver
    let mut caps = DesiredCapabilities::firefox();
    caps.set_page_load_strategy(thirtyfour::PageLoadStrategy::Eager)
        .unwrap();
    caps.set_proxy(thirtyfour::Proxy::Manual {
        http_proxy: Some(format!("127.0.0.1:{}", args.proxy_port)),
        ssl_proxy: Some(format!("127.0.0.1:{}", args.proxy_port)),
        socks_proxy: None,
        socks_version: None,
        socks_username: None,
        socks_password: None,
        no_proxy: None,
        ftp_proxy: None,
    })
    .unwrap();
    caps.accept_insecure_certs(true).unwrap();
    let wd = match WebDriver::new(args.geckodriver_address, caps).await {
        Ok(wd) => wd,
        Err(e) => {
            tracing::error!(
                "Seems that geckodriver is not started or wrong port is set up: {e}"
            );
            exit(4);
        }
    };

    // Prepare paths
    let base = &args.base;
    let root = &args.root;
    let domain = &args.domain;

    load_root_page(
        &wd,
        domain,
        root,
        &args.password,
        &args.email,
        &args.auth_url,
    )
    .await?;

    let pagename = wd.title().await.unwrap_or("NotNamedPage".to_string());
    let filepath = vec![pagename];

    let dom = wd.source().await.unwrap();

    let mut checked = HashSet::new();
    checked.insert(Href(String::from(base)));
    checked.insert(Href(String::from(root)));

    let mut hrefs = Href::from_document(&dom, &base);
    hrefs.swap(0, 1);

    print_err!(
        process_selectors(
            &wd,
            &domain,
            &base,
            hrefs,
            &mut checked,
            filepath,
            interceptor_tx,
            saver_tx,
        )
        .await,
        ()
    );

    wait_video_saver(video_saver_handle).await;

    // Close webdriver session
    wd.quit().await.unwrap();

    Ok(())
}

fn process_selectors<'a>(
    wd: &'a WebDriver,
    domain: &'a str,
    base: &'a str,
    hrefs: Vec<Href>,
    checked: &'a mut HashSet<Href>,
    filepath: Vec<String>,
    tx: Sender<Signal>,
    saver_tx: Sender<VideoInfo>,
) -> BoxFuture<'a, Result<(), anyhow::Error>> {
    async move {
        for href in hrefs.iter() {
            if checked.contains(&href) {
                continue;
            }
            checked.insert(href.clone());

            if let Err(e) =
                wd.goto(&format!("{}:{}", domain, href.as_ref())).await
            {
                tracing::error!(
                    "Failed to navigate to: {:?}, error: {e}",
                    href
                );
                continue;
            } else {
                let dom = wd.source().await.unwrap();
                if does_page_contains_videos(&dom) {
                    tracing::info!(
                        "Found page contains videos: {}",
                        merge_path(&filepath)
                    );
                    print_err!(
                        store_video(
                            wd,
                            tx.clone(),
                            &filepath,
                            saver_tx.clone()
                        )
                        .await,
                        ()
                    );
                }
                let mut filepath = filepath.clone();
                filepath.push(
                    wd.title().await.unwrap_or("NotNamedPage".to_string()),
                );

                // Recursion
                print_err!(
                    process_selectors(
                        wd,
                        domain,
                        base,
                        Href::from_document(&dom, base),
                        checked,
                        filepath,
                        tx.clone(),
                        saver_tx.clone(),
                    )
                    .await,
                    ()
                );
            }
        }
        Ok(())
    }
    .boxed()
}

async fn store_video(
    wd: &WebDriver,
    tx: Sender<Signal>,
    filepath: &Vec<String>,
    saver_tx: Sender<VideoInfo>,
) -> Result<(), anyhow::Error> {
    for element in wd.find_all(By::Css("iframe.vhi-iframe")).await? {
        // Go into iframe
        element.wait_until().displayed().await?;
        element.enter_frame().await?;

        //Try to find one of two tags
        let sleep = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(sleep);
        tx.send(Signal::StartListening).await?;
        loop {
            tokio::select! {
                biased;
                Ok(button) = wd.find(By::Css("button.cnf-button--confirm")) => {
                    print_err!(button.click().await, ());
                    break;
                }
                () = &mut sleep, if !sleep.is_elapsed() => {
                    tracing::info!("Search for confirm button is timed out");
                }
                Ok(player_root) = wd.find(By::Id("player-root")), if sleep.is_elapsed() => {
                    print_err!(player_root.click().await, ());
                    break;
                }
                else => {
                    tracing::warn!("Can't find player-root or confirm-button, trying again");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }

        // Wait for urls collection in proxy
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Fetch urls from proxy
        let (one_tx, one_rx) = tokio::sync::oneshot::channel();
        tx.send(Signal::StopListening(one_tx)).await?;
        match one_rx.await {
            Ok(urls) => {
                saver_tx
                    .send(VideoInfo {
                        path: merge_path(filepath),
                        urls,
                    })
                    .await?;
            }
            Err(e) => tracing::error!("Failed to get urls: {e}"),
        }
        // Go back from iframe
        wd.enter_parent_frame().await.unwrap();
    }
    Ok(())
}

async fn run_proxy() -> Result<Sender<Signal>, anyhow::Error> {
    let (tx, rx) = tokio::sync::mpsc::channel(10000);
    proxy::run_interceptor(rx).await;
    Ok(tx)
}

async fn wait_video_saver(handle: JoinHandle<Vec<(VideoInfo, anyhow::Error)>>) {
    match handle.await {
        Ok(failed) => {
            if failed.is_empty() {
                tracing::info!(
                    "There are no failed videos to download, congratulations!"
                );
            } else {
                tracing::warn!("Got failed videos to download: {:?}", failed);
                match serde_json::to_string_pretty(
                    &failed
                        .into_iter()
                        .map(|(info, _)| info)
                        .collect::<Vec<VideoInfo>>(),
                ) {
                    Ok(s) => print_err!(
                        write_file("failed_videos_data.json", &s),
                        ()
                    ),
                    Err(e) => tracing::error!(
                        "Failed to serialize failed videos data: {e}"
                    ),
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to join video saver: {}", e);
        }
    }
}

// ───── Helpers ──────────────────────────────────────────────────────────── //

async fn load_root_page(
    wd: &WebDriver,
    domain: &str,
    root: &str,
    password: &str,
    email: &str,
    auth_url: &str,
) -> Result<(), anyhow::Error> {
    match read_cookies() {
        Ok(cookies) => {
            wd.goto(&format!("{}:{}", domain, root)).await?;
            for cookie in cookies {
                wd.add_cookie(cookie).await?;
            }
            wd.goto(&format!("{}:{}", domain, root)).await?;
        }
        Err(e) => {
            tracing::info!("Failed to read cookies: {e}");
            wd.goto(auth_url).await?;
            wd.find(By::Css("input.form-field-email"))
                .await?
                .send_keys(email)
                .await?;
            wd.find(By::Css("input.form-field-password"))
                .await?
                .send_keys(password)
                .await?;
            wd.find(By::Css("button.btn-success-sech"))
                .await?
                .click()
                .await?;

            let cookies = wd.get_all_cookies().await?;
            store_cookies(cookies)?;
        }
    }
    Ok(())
}

fn store_cookies(cookies: Vec<Cookie>) -> anyhow::Result<()> {
    let cookies = serde_json::to_string(&cookies)?;
    write_file("cookie.txt", &cookies)
}

fn write_file(filepath: &str, content: &str) -> anyhow::Result<()> {
    let mut output = File::create(filepath).unwrap();
    write!(output, "{}", content)?;
    Ok(())
}

fn read_cookies() -> anyhow::Result<Vec<Cookie>> {
    let cookie_str = read_to_string("cookie.txt")?;
    Ok(serde_json::from_str(&cookie_str)?)
}

fn does_page_contains_videos(html: &str) -> bool {
    html.contains("vhplayeriframe")
}

fn merge_path(filepath: &Vec<String>) -> String {
    filepath
        .iter()
        .fold(Default::default(), |mut acc: String, elem| {
            acc += "/";
            acc += elem.as_str();
            acc
        })[1..]
        .to_string()
}
