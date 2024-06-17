use std::fs::File;
use std::io::BufWriter;
use std::str::FromStr;
use std::sync::OnceLock;
use std::{path::PathBuf, process::Stdio};

use anyhow::anyhow;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Receiver;

fn urls_regex() -> &'static Regex {
    static HREF_REGEX: OnceLock<Regex> = OnceLock::new();
    HREF_REGEX.get_or_init(|| Regex::new(r#"(?P<domain>[^:]+):[0-9]+\/player\/(?P<id>[^\/]+)\/[^\/]+(\/media)?\/(?P<file>[^?]+)\.m3u8.*"#).unwrap())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VideoInfo {
    pub path: String,
    pub urls: Vec<String>,
}

impl FromStr for VideoInfo {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

pub struct VideoSaver {
    rx: Receiver<VideoInfo>,
    failed: Vec<(VideoInfo, anyhow::Error)>,
    ffmpeg_output: File,
}

impl VideoSaver {
    pub fn new(rx: Receiver<VideoInfo>) -> Self {
        let ffmpeg_output = File::options()
            .create(true)
            .append(true)
            .write(true)
            .open("ffmpeg.log")
            .expect("Can't open ffmpeg log file!");
        VideoSaver {
            rx,
            failed: Vec::new(),
            ffmpeg_output,
        }
    }

    pub fn run_video_saver(
        mut self,
    ) -> tokio::task::JoinHandle<Vec<(VideoInfo, anyhow::Error)>> {
        tokio::spawn(async move {
            while let Some(video_info) = self.rx.recv().await {
                tracing::info!(
                    "Got video info, start downloading, currently in queue: {}",
                    self.rx.len()
                );
                match self.write_file(video_info).await {
                    Ok(path) => tracing::info!(
                        "Succesfully downloaded file: {}",
                        path.to_string_lossy()
                    ),
                    Err(e) => {
                        tracing::error!("Failed to download video: {}", e.1);
                        self.failed.push(e)
                    }
                }
            }
            self.failed
        })
    }

    async fn write_file(
        &self,
        video_info: VideoInfo,
    ) -> Result<PathBuf, (VideoInfo, anyhow::Error)> {
        let url = select_url(video_info.urls.clone())
            .map_err(|e| (video_info.clone(), e))?
            .extract();
        let path = PathBuf::from(format!("./{}", video_info.path));
        if let Err(err) = std::fs::create_dir_all(&path) {
            tracing::error!("Error creating directory: {}", err);
        } else {
            tracing::info!("Directory created successfully!");
        }

        let filepath = path.join(format!("{}.mp4", hash_string(&url)));

        if filepath.exists() {
            tracing::warn!(
                "File already exists, skip: {}",
                filepath.to_string_lossy()
            );
            return Ok(filepath);
        }

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
            .stdout(self.ffmpeg_output.try_clone().map_err(|e| {
                (
                    video_info.clone(),
                    anyhow!("Failed to clone ffmpeg stdout file handle: {e}"),
                )
            })?)
            .stderr(self.ffmpeg_output.try_clone().map_err(|e| {
                (
                    video_info.clone(),
                    anyhow!("Failed to clone ffmpeg stderr file handle: {e}"),
                )
            })?)
            .spawn()
            .map_err(|e| (video_info.clone(), anyhow!("Failed: {e}")))?;
        let status = child
            .wait()
            .await
            .map_err(|e| (video_info.clone(), anyhow!("Failed: {e}")))?;
        if status.success() {
            Ok(filepath)
        } else {
            Err((video_info, anyhow!("Failed: exited with nonzero code")))
        }
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

fn hash_string(s: &str) -> String {
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(s, &mut hasher);
    std::hash::Hasher::finish(&hasher).to_string()
}

#[cfg(test)]
mod tests {
    use super::select_url;

    #[test]
    fn testme() {
        let urls = vec!["https://player02.getcourse.ru:443/player/211deb093b64e367becbdd4d130e0a29/2b0d302f8e569d2ce309d4429e518dd8/media/360.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A0%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/211deb093b64e367becbdd4d130e0a29/2b0d302f8e569d2ce309d4429e518dd8/media/580.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A0%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/211deb093b64e367becbdd4d130e0a29/2b0d302f8e569d2ce309d4429e518dd8/media/480.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A0%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/d577fb479e3afb177663fdc27a90a46b/bcedaa97e91a4e83f336ee76904cd537/master.m3u8?user-cdn=cdnvideo&acc-id=253685&user-id=221868265&loc-mode=ru&version=10%3A2%3A1%3A1%3A2%3Acdnvideo&consumer=vod&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/a3404f00e961b8dffd373b0dab2b4559/87e7ec1b5a2a2f2a4f32bfb3c2e2312c/master.m3u8?user-cdn=integrosproxy&acc-id=253685&user-id=221868265&loc-mode=ru&version=10%3A2%3A1%3A1%3A2%3Aintegrosproxy&consumer=vod&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/2d42f5470d4861b544643414ac32f1f3/ec79d2a18ad2cfbf03ad271f056f2516/master.m3u8?user-cdn=cdnvideo&acc-id=253685&user-id=221868265&loc-mode=ru&version=10%3A2%3A1%3A1%3A2%3Acdnvideo&consumer=vod&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/55643edf6feaa2ad978de6f876cde83a/623dee040a7b8492a34a73b26b2e4032/master.m3u8?user-cdn=cdnvideo&acc-id=253685&user-id=221868265&loc-mode=ru&version=10%3A2%3A1%3A1%3A2%3Acdnvideo&consumer=vod&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/d577fb479e3afb177663fdc27a90a46b/bcedaa97e91a4e83f336ee76904cd537/media/360.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A1%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string(),
                        "https://player02.getcourse.ru:443/player/d577fb479e3afb177663fdc27a90a46b/bcedaa97e91a4e83f336ee76904cd537/media/480.m3u8?sid=&user-cdn=cdnvideo&version=10%3A2%3A1%3A1%3Acdnvideo&user-id=221868265&jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1c2VyLWlkIjoyMjE4NjgyNjV9.0piDRlkDE13G3KXLTLopsIPSGeIXa8f3zyACc1aqzNU".to_string()];

        let selected = select_url(urls).unwrap();
        dbg!(selected);
    }
}
