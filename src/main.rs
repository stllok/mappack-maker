use std::io::Cursor;

use anyhow::{anyhow, Result};
use async_zip::{tokio::write::ZipFileWriter, Compression, ZipEntryBuilder};
use bytes::Bytes;
use dotenvy::dotenv;
use futures::{stream, StreamExt, TryFutureExt, TryStreamExt};
use reqwest::{header::CONTENT_DISPOSITION, Client, Response};
use source::{MirrorSource, SayoBotMinimum, SayoBotServer};
use tokio::io::AsyncWriteExt;

mod source;

#[macro_export]
macro_rules! regex_once {
    ($re:literal $(,)?) => {{
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

async fn handle_download(
    map_id: u32,
    mirror: &MirrorSource,
    cli: &Client,
) -> Result<(ZipEntryBuilder, Bytes)> {
    println!("downloading {map_id}");
    mirror
        .as_request(map_id, cli)
        .and_then(|req| cli.execute(req).map_err(Into::into))
        .map_ok(|res| {
            (
                res.headers()
                    .get(CONTENT_DISPOSITION)
                    .and_then(|val| {
                        regex_once!(r#"(?:filename=")(.*)""#)
                            .captures(val.to_str().unwrap_or_default())
                    })
                    .and_then(|val| val.get(1))
                    .map(|val| val.as_str().replace("/", "_").to_string())
                    .unwrap_or(map_id.to_string()),
                res,
            )
        })
        .and_then(|(filename, res)| async move {
            res.error_for_status()
                .map(|res| (res, filename))
                .map_err(Into::into)
        })
        .and_then(|(res, filename)| {
            Response::bytes(res)
                .map_ok(|res| (res, filename))
                .map_err(Into::into)
        })
        .map_ok(|(data, filename)| {
            (
                ZipEntryBuilder::new(filename.into(), Compression::Deflate),
                data,
            )
        })
        .inspect_err(|_| println!("{map_id} download failed!"))
        .inspect_ok(|_| println!("finished download {map_id}"))
        .await
}

async fn attempt_download(
    map_id: u32,
    mirrors: &[MirrorSource],
    cli: &Client,
) -> Result<(ZipEntryBuilder, Bytes)> {
    for mirror in mirrors {
        let res = handle_download(map_id, mirror, cli).await;

        if let Err(e) = &res {
            eprintln!("mapid: {map_id}; ERROR: {e:?}");
        } else {
            return res;
        }
    }

    Err(anyhow!("None of mirrors work for {map_id}"))
}

async fn make_mappack(map_ids: Vec<u32>, mirrors: Vec<MirrorSource>, limit: u8) -> Result<()> {
    let client = Client::builder().build()?;

    let maps = stream::iter(map_ids)
        .map(|id| attempt_download(id, &mirrors, &client))
        .buffer_unordered(limit as usize)
        .try_collect::<Vec<(ZipEntryBuilder, Bytes)>>()
        .await?;

    println!("Download complete, packing...");
    let mut bytes = vec![];
    let mut cursor = Cursor::new(&mut bytes);
    let mut writer = ZipFileWriter::with_tokio(&mut cursor);
    for (builder, b) in maps {
        writer.write_entry_whole(builder, &b).await?;
    }
    writer.close().await?;

    tokio::fs::File::create("/tmp/mappack.zip")
        .await?
        .write_all(&bytes)
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().expect(".env file not found");
    // test_bancho(&env::var("BANCHO_COOKIE").unwrap()).await
    let limit = 5;
    let maps = std::fs::read_to_string("input.txt")
        .map_err(Into::into)
        .and_then(|s| {
            s.trim()
                .split('\n')
                .map(|id| id.parse::<u32>().map_err(Into::into))
                .collect::<Result<Vec<u32>, anyhow::Error>>()
        })?;
    make_mappack(
        maps,
        vec![
            MirrorSource::Bancho { no_video: false },
            MirrorSource::SayoBot {
                level: SayoBotMinimum::NoVideo,
                mirror: SayoBotServer::TencentYunCDN,
            },
            MirrorSource::SayoBot {
                level: SayoBotMinimum::NoVideo,
                mirror: SayoBotServer::Auto,
            },
            MirrorSource::NeriNyan { no_video: true },
            MirrorSource::BeatConnect,
        ],
        limit,
    )
    .await
}
