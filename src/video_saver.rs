use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, Context};
use regex::Regex;
use tokio::sync::mpsc::Receiver;
use tokio::time::timeout;

fn urls_regex() -> &'static Regex {
    static HREF_REGEX: OnceLock<Regex> = OnceLock::new();
    HREF_REGEX.get_or_init(|| Regex::new(r#"(?P<domain>[^:]+):[0-9]+\/player\/(?P<id>[^\/]+)\/[^\/]+\/media\/(?P<file>[^?]+)\.m3u8.*"#).unwrap())
}

#[derive(Clone, Debug)]
pub struct VideoInfo {
    pub path: String,
    pub name: String,
    pub urls: Vec<String>,
}

pub struct VideoSaver {
    rx: Receiver<VideoInfo>,
    failed: Vec<(VideoInfo, anyhow::Error)>,
}

impl VideoSaver {
    pub fn new(rx: Receiver<VideoInfo>) -> Self {
        VideoSaver {
            rx,
            failed: Vec::new(),
        }
    }

    pub fn run_video_saver(
        mut self,
    ) -> tokio::task::JoinHandle<Vec<(VideoInfo, anyhow::Error)>> {
        tokio::spawn(async move {
            while let Some(video_info) = self.rx.recv().await {
                println!("Got video info, start downloading");
                match write_file(video_info).await {
                    Ok(()) => println!("Succesfully downloaded file"),
                    Err(e) => {
                        println!("Failed to download video: {}", e.1);
                        self.failed.push(e)
                    }
                }
            }
            self.failed
        })
    }
}

async fn write_file(
    video_info: VideoInfo,
) -> Result<(), (VideoInfo, anyhow::Error)> {
    let url = select_url(video_info.urls.clone())
        .map_err(|e| (video_info.clone(), e))?
        .extract();
    let path = PathBuf::from(format!("./{}", video_info.path));
    if let Err(err) = std::fs::create_dir_all(&path) {
        eprintln!("Error creating directory: {}", err);
    } else {
        println!("Directory created successfully!");
    }

    let filepath = path.join(video_info.name.clone());

    let mut child = tokio::process::Command::new("ffmpeg")
        .args(&[
            "-protocol_whitelist",
            "file,http,https,tcp,tls",
            "-i",
            &url,
            "-c",
            "copy",
            filepath.to_str().unwrap(),
        ])
        .spawn()
        .map_err(|e| (video_info.clone(), anyhow!("Failed: {e}")))?;
    let status = child
        .wait()
        .await
        .map_err(|e| (video_info.clone(), anyhow!("Failed: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err((video_info, anyhow!("Failed: exited with nonzero code")))
    }
}

#[derive(Debug)]
enum TypedUrl {
    Text(String),
    Number(usize, String),
}

impl TypedUrl {
    fn extract(self) -> String {
        match self {
            TypedUrl::Text(s) => s,
            TypedUrl::Number(_, s) => s,
        }
    }
}

impl std::cmp::PartialEq for TypedUrl {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TypedUrl::Text(_), TypedUrl::Text(_)) => true,
            (TypedUrl::Text(_), TypedUrl::Number(_, _)) => false,
            (TypedUrl::Number(_, _), TypedUrl::Text(_)) => false,
            (TypedUrl::Number(n1, _), TypedUrl::Number(n2, _)) => n1 == n2,
        }
    }
}

impl std::cmp::Eq for TypedUrl {}

impl std::cmp::PartialOrd for TypedUrl {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (TypedUrl::Text(_), TypedUrl::Text(_)) => {
                Some(std::cmp::Ordering::Equal)
            }
            (TypedUrl::Text(_), TypedUrl::Number(_, _)) => {
                Some(std::cmp::Ordering::Greater)
            }
            (TypedUrl::Number(_, _), TypedUrl::Text(_)) => {
                Some(std::cmp::Ordering::Less)
            }
            (TypedUrl::Number(n1, _), TypedUrl::Number(n2, _)) => {
                Some(n1.cmp(n2))
            }
        }
    }
}

impl std::cmp::Ord for TypedUrl {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

fn select_url(urls: Vec<String>) -> Result<TypedUrl, anyhow::Error> {
    if urls.is_empty() {
        return Err(anyhow!("Urls vector is emtpy"));
    }

    let collection = urls
        .into_iter()
        .map(|url| {
            let captures = urls_regex()
                .captures(&url)
                .ok_or(anyhow!("Failed to capture regex in url: {url}"))?;
            let file_name = captures.name("file").unwrap().as_str();
            match file_name.parse() {
                Ok(num) => {
                    Ok::<TypedUrl, anyhow::Error>(TypedUrl::Number(num, url))
                }
                Err(_) => Ok::<TypedUrl, anyhow::Error>(TypedUrl::Text(url)),
            }
        })
        .collect::<Result<Vec<TypedUrl>, anyhow::Error>>()?;

    collection
        .into_iter()
        .max()
        .ok_or(anyhow!("Failed to find highest priority url"))
}

#[cfg(test)]
mod tests {
    use super::select_url;

    #[test]
    fn testme() {
        let urls = vec!["https://player02.getcourse.ru:443/player/211deb093b64e367becbdd4d130e0a29/2b0d302f8e569d2ce309d4429e518dd8/media/360.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A0%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/211deb093b64e367becbdd4d130e0a29/2b0d302f8e569d2ce309d4429e518dd8/media/580.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A0%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/211deb093b64e367becbdd4d130e0a29/2b0d302f8e569d2ce309d4429e518dd8/media/480.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A0%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string()];
        let selected = select_url(urls).unwrap();
        dbg!(selected);
    }
}
