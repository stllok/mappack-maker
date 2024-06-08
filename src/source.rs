use std::{
    env::{self, VarError},
    sync::OnceLock,
    time::Duration,
};

use anyhow::{anyhow, Result};
use futures::{FutureExt, TryFutureExt};
use reqwest::{
    header::{self, COOKIE, REFERER, USER_AGENT},
    Client, Method, Request, RequestBuilder,
};
use tokio::sync::{OnceCell, Semaphore};

static BANCHO_COOKIE: OnceCell<String> = OnceCell::const_new();
const COMMON_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/42.0.2311.135 Safari/537.36 Edge/12.246";

async fn get_bancho_cookie() -> Result<&'static str> {
    BANCHO_COOKIE
        .get_or_try_init(|| async { env::var("BANCHO_COOKIE") })
        .map_ok(|s| s.as_str())
        .map_err(Into::into)
        .await
}

#[derive(Debug, Clone, Copy)]
pub enum SayoBotMinimum {
    Full,
    NoVideo,
    Mini,
}

impl AsRef<str> for SayoBotMinimum {
    fn as_ref(&self) -> &str {
        match self {
            SayoBotMinimum::Full => "full",
            SayoBotMinimum::NoVideo => "novideo",
            SayoBotMinimum::Mini => "mini",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SayoBotServer {
    Auto,
    ChinaTelecom,
    ChinaMobile,
    ChinaUnicom,
    TencentYunCDN,
    Generamy,
    American,
}

impl AsRef<str> for SayoBotServer {
    fn as_ref(&self) -> &str {
        match self {
            SayoBotServer::Auto => "auto",
            SayoBotServer::ChinaTelecom => "Telecom",
            SayoBotServer::ChinaMobile => "cmcc",
            SayoBotServer::ChinaUnicom => "unicom",
            SayoBotServer::TencentYunCDN => "CDN",
            SayoBotServer::Generamy => "DE",
            SayoBotServer::American => "USA",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MirrorSource {
    Bancho {
        no_video: bool,
    },
    BeatConnect,
    SayoBot {
        level: SayoBotMinimum,
        mirror: SayoBotServer,
    },
    NeriNyan {
        no_video: bool,
    },
    ChimuMoe {
        no_video: bool,
    },
}

// Prevent bancho Cloudflare Bot spam trigger
static RATELIMITER_ON_REQUEST: Semaphore = Semaphore::const_new(1);
impl MirrorSource {
    pub async fn as_request(self, map_id: u32, cli: &Client) -> Result<Request> {
        let lock = RATELIMITER_ON_REQUEST.acquire().await?;

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(lock);
        });

        match self {
            MirrorSource::Bancho { no_video } => {
                let url = format!("https://osu.ppy.sh/beatmapsets/{map_id}/download");
                cli.get(&url)
                    .query(&[("noVideo", if no_video { 1 } else { 0 })])
                    .header(USER_AGENT, COMMON_USER_AGENT)
                    .header(REFERER, &url)
                    .header(COOKIE, get_bancho_cookie().await?)
                    .build()
            }
            MirrorSource::BeatConnect => cli
                .get(format!("https://beatconnect.io/b/{map_id}"))
                .header(USER_AGENT, COMMON_USER_AGENT)
                .build(),
            MirrorSource::SayoBot { level, mirror } => cli
                .get(format!(
                    "https://dl.sayobot.cn/beatmaps/download/{}/{map_id}",
                    level.as_ref(),
                ))
                .query(&[("server", mirror.as_ref())])
                .header(USER_AGENT, COMMON_USER_AGENT)
                .build(),
            MirrorSource::NeriNyan { no_video } => cli
                .get(format!("https://api.nerinyan.moe/d/{map_id}",))
                .query(&[("nv", if no_video { 1 } else { 0 })])
                .header(USER_AGENT, COMMON_USER_AGENT)
                .build(),
            MirrorSource::ChimuMoe { no_video } => cli
                .get(format!("https://api.chimu.moe/v1/download/{map_id}",))
                .query(&[("n", if no_video { 1 } else { 0 })])
                .header(USER_AGENT, COMMON_USER_AGENT)
                .build(),
        }
        .map_err(Into::into)
    }
}
