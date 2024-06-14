use std::collections::HashSet;
use std::error::Error;
use std::fs::{read_to_string, File};
use std::io::Write;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use fantoccini::wd::{
    Capabilities, TimeoutConfiguration, WebDriverCompatibleCommand,
};
use futures::future::{BoxFuture, FutureExt};

use fantoccini::Client;
use regex::Regex;
use thirtyfour::common::command::Command;
use thirtyfour::extensions::query::ElementWaitable;
use thirtyfour::{
    By, CapabilitiesHelper, Cookie, DesiredCapabilities, WebDriver,
};
use webdriver::command::WebDriverCommand;

fn href_regex() -> &'static Regex {
    static HREF_REGEX: OnceLock<Regex> = OnceLock::new();
    HREF_REGEX.get_or_init(|| Regex::new(r#"href="([^"]+/([^"]+))""#).unwrap())
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
struct Href(String);

impl AsRef<str> for Href {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Href {
    fn from_document(html: &str, base: &str) -> Vec<Href> {
        let r = href_regex()
            .captures_iter(html)
            .map(|c| {
                let path = c.get(1).unwrap().as_str().to_string();
                let last = c.get(2).unwrap().as_str();
                if last.contains(['.', '?']) || !path.starts_with(base) {
                    None
                } else {
                    Some(Href(path))
                }
            })
            .flatten()
            .collect();
        r
    }
}

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn Error>> {
    let mut caps = DesiredCapabilities::firefox();
    caps.set_page_load_strategy(thirtyfour::PageLoadStrategy::Eager)
        .unwrap();
    // url: String::from("127.0.0.1:8080"),
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

    let base = "/teach/control";
    let root = "/teach/control/stream/index";
    let domain = "https://universkill.ru";

    load_root_page(&wd, domain, root).await?;

    // let r = "https://universkill.ru/pl/teach/control/lesson/view?id=253679334&editMode=0";

    // wd.goto(r).await.unwrap();

    // store_video(&wd).await;

    // wd.close().await.unwrap();
    // return Ok(());

    let pagename = wd.title().await.unwrap_or("NotNamedPage".to_string());
    let filepath = vec![pagename];

    let dom = wd.source().await.unwrap();

    let mut checked = HashSet::new();
    checked.insert(Href(String::from(base)));
    checked.insert(Href(String::from(root)));

    let mut hrefs = Href::from_document(&dom, &base);
    hrefs.swap(0, 1);
    if let Err(e) = process_selectors(
        &wd,
        &domain,
        &base,
        &root,
        hrefs,
        &mut checked,
        filepath,
    )
    .await
    {
        println!("Failed to process sels on root, error: {e}");
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    // Close webdriver session
    wd.close().await.unwrap();

    Ok(())
}

fn process_selectors<'a>(
    wd: &'a WebDriver,
    domain: &'a str,
    base: &'a str,
    parent: &'a str,
    mut hrefs: Vec<Href>,
    checked: &'a mut HashSet<Href>,
    filepath: Vec<String>,
) -> BoxFuture<'a, Result<(), anyhow::Error>> {
    async move {
        print_current_path(&filepath);
        hrefs.reverse();
        for href in hrefs {
            if checked.contains(&href) {
                continue;
            } else {
                println!("Trying {}", href.as_ref());
                checked.insert(href.clone());
            }
            if let Err(e) =
                wd.goto(&format!("{}:{}", domain, href.as_ref())).await
            {
                println!("Failed to navigate to: {:?}, error: {e}", href);
                continue;
            } else {
                let dom = wd.source().await.unwrap();
                if does_page_contains_videos(&dom) {
                    println!("Found video, storing it");
                    store_video(wd).await;
                }
                let mut filepath = filepath.clone();
                filepath.push(
                    wd.title().await.unwrap_or("NotNamedPage".to_string()),
                );
                if let Err(e) = process_selectors(
                    wd,
                    domain,
                    base,
                    href.as_ref(),
                    Href::from_document(&dom, base),
                    checked,
                    filepath,
                )
                .await
                {
                    println!("Failed to process sels, error: {e}");
                }
            }
        }
        println!("End cycle step");
        Ok(())
    }
    .boxed()
}

async fn load_root_page(
    wd: &WebDriver,
    domain: &str,
    root: &str,
) -> Result<(), anyhow::Error> {
    if let Ok(cookies) = read_cookies() {
        wd.goto(&format!("{}:{}", domain, root)).await?;
        for cookie in cookies {
            wd.add_cookie(cookie).await?;
        }
        wd.goto(&format!("{}:{}", domain, root)).await?;
    } else {
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

fn print_current_path(filepath: &Vec<String>) {
    let path =
        &filepath
            .iter()
            .fold(Default::default(), |mut acc: String, elem| {
                acc += "/";
                acc += elem.as_str();
                acc
            })[1..];
    println!("Path: {path}");
}

async fn store_video(wd: &WebDriver) {
    println!("Storing video");
    let elements = wd.find_all(By::Css("iframe.vhi-iframe")).await.unwrap();
    dbg!(&elements);
    for element in elements {
        println!("{}", element.to_json().unwrap());
        element.wait_until().displayed().await.unwrap();
        element.enter_frame().await.unwrap();
        let sleep = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                biased;
                Ok(button) = wd.find(By::Css("button.cnf-button--confirm")) => {
                    if let Err(e) = button.click().await {
                        println!("Error: {e}");
                    }
                        break;
                }
                _ = &mut sleep, if !sleep.is_elapsed() => {
                    println!("operation timed out");
                }
                Ok(player_root) = wd.find(By::Id("player-root")), if sleep.is_elapsed() => {
                    if let Err(e) = player_root.click().await {
                        println!("Error: {e}");
                    }
                        break;
                }
            }
        }
        println!("Waiting for video");
        tokio::time::sleep(Duration::from_secs(2)).await;
        wd.enter_parent_frame().await.unwrap();
    }
}
