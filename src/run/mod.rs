use std::collections::HashSet;
use std::error::Error;
use std::fs::{read_to_string, File};
use std::io::Write;
use std::time::Duration;

use futures::future::{BoxFuture, FutureExt};
use thirtyfour::extensions::query::ElementWaitable;
use thirtyfour::{
    By, CapabilitiesHelper, Cookie, DesiredCapabilities, WebDriver,
};
use tokio::sync::mpsc::Sender;

use crate::print_err;
use crate::proxy::{self, Signal};
use crate::video_saver::{VideoInfo, VideoSaver};

use self::href::Href;

mod href;

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn Error>> {
    // Prepare communication
    let interceptor_tx = run_proxy().await.unwrap();
    let (saver_tx, saver_rx) = tokio::sync::mpsc::channel(10000);
    let video_saver = VideoSaver::new(saver_rx);
    let video_saver_handle = video_saver.run_video_saver();

    // Setup webdriver
    let mut caps = DesiredCapabilities::firefox();
    caps.set_page_load_strategy(thirtyfour::PageLoadStrategy::Eager)
        .unwrap();
    caps.set_proxy(thirtyfour::Proxy::Manual {
        http_proxy: Some("127.0.0.1:8080".to_string()),
        ssl_proxy: Some("127.0.0.1:8080".to_string()),
        socks_proxy: None,
        socks_version: None,
        socks_username: None,
        socks_password: None,
        no_proxy: None,
        ftp_proxy: None,
    })
    .unwrap();
    caps.accept_insecure_certs(true).unwrap();
    let wd = WebDriver::new("http://localhost:4444", caps).await?;

    // Prepare paths
    let base = "/teach/control";
    let root = "/teach/control/stream/index";
    let domain = "https://universkill.ru";

    load_root_page(&wd, domain, root).await?;

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

    match video_saver_handle.await {
        Ok(failed) => {
            if failed.is_empty() {
                println!(
                    "There are no failed videos to download, congratulations!"
                );
            } else {
                println!("Got failed videos to download: {:?}", failed);
            }
        }
        Err(e) => {
            println!("Failed to join video saver: {e}");
        }
    }

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
                println!("Failed to navigate to: {:?}, error: {e}", href);
                continue;
            } else {
                let dom = wd.source().await.unwrap();
                if does_page_contains_videos(&dom) {
                    println!("Found video: {}", merge_path(&filepath));
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
                _ = &mut sleep, if !sleep.is_elapsed() => {
                    println!("operation timed out");
                }
                Ok(player_root) = wd.find(By::Id("player-root")), if sleep.is_elapsed() => {
                    print_err!(player_root.click().await, ());
                    break;
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
                        name: format!("{}.mp4", uuid::Uuid::new_v4()),
                        urls,
                    })
                    .await?;
            }
            Err(e) => println!("Failed to get urls: {e}"),
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

// ───── Helpers ──────────────────────────────────────────────────────────── //

async fn load_root_page(
    wd: &WebDriver,
    domain: &str,
    root: &str,
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
            println!("Failed to read cookies: {e}");
            let unauthorized_url =
                "https://universkill.ru/cms/system/login?required=true";
            wd.goto(unauthorized_url).await?;
            println!("Waiting for email field");
            wd.find(By::Css("input.form-field-email"))
                .await?
                .send_keys("")
                .await?;
            println!("Waiting for password field");
            wd.find(By::Css("input.form-field-password"))
                .await?
                .send_keys("")
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
    let mut output = File::create("cookie.txt").unwrap();
    write!(output, "{}", cookies)?;
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
