extern crate chrono;
extern crate failure;
extern crate hyper;
extern crate rand;
extern crate serde;
extern crate serde_json;
extern crate text_io;
extern crate tokio;
extern crate unrar;
extern crate url;
extern crate zip;

use chrono::Local;
use hyper::rt::Future;
use hyper::rt::Stream;
use hyper::Client;
use rand::Rng;
use serde::Deserialize;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use text_io::*;
use tokio::runtime::Runtime;
use unrar::Archive;
use url::Url;
use zip::read::ZipArchive;

macro_rules! notice {
    ($($arg:tt)*) => ({
        std::io::stdout().write(
            format!("{}{}\n",
                format!("[{}][NOTICE][{}:{}]", Local::now().format("%Y%m%d %H:%M:%S %6f"), file!(), line!()),
                format!($($arg)*)
            ).as_bytes()
        ).unwrap();
    })
}

#[derive(Deserialize, Debug)]
struct Subs {
    id: u32,
    native_name: String,
    revision: u32,
    upload_time: String,
    subtype: String,
    vote_score: u32,
    #[serde(default)]
    release_site: String,
    #[serde(default)]
    videoname: String,
    #[serde(default)]
    vote_machine_translate: String,
    #[serde(default)]
    url: String,
}

#[derive(Deserialize, Debug)]
struct Res {
    status: u32,
    sub: Sub,
}

#[derive(Deserialize, Debug)]
struct Sub {
    #[serde(default)]
    keyword: String,
    result: String,
    subs: Vec<Subs>,
    action: String,
}

// Define a type so we can return multiple types of errors
#[derive(Debug)]
enum FetchError {
    Http(hyper::Error),
    Json(serde_json::Error),
    Zip(zip::result::ZipError),
    IO(std::io::Error),
    Failure(failure::Error),
    Logic(String),
}

impl From<failure::Error> for FetchError {
    fn from(err: failure::Error) -> FetchError {
        FetchError::Failure(err)
    }
}

impl From<std::io::Error> for FetchError {
    fn from(err: std::io::Error) -> FetchError {
        FetchError::IO(err)
    }
}

impl From<hyper::Error> for FetchError {
    fn from(err: hyper::Error) -> FetchError {
        FetchError::Http(err)
    }
}

impl From<serde_json::Error> for FetchError {
    fn from(err: serde_json::Error) -> FetchError {
        FetchError::Json(err)
    }
}

impl From<zip::result::ZipError> for FetchError {
    fn from(err: zip::result::ZipError) -> FetchError {
        FetchError::Zip(err)
    }
}

fn download(video_file: String, keyword: &String) -> bool {
    notice!("search {}", keyword);
    let mut rt = Runtime::new().unwrap();
    let token = "n857dxlJNYYmHuX0kH5eN65oJ1b8pIba";
    let client = Client::new();
    let mut search_uri: Url = "http://api.assrt.net/v1/sub/search".parse().unwrap();
    let mut detail_uri: Url = "http://api.assrt.net/v1/sub/detail".parse().unwrap();
    {
        let mut query = search_uri.query_pairs_mut();
        query
            .append_pair("token", token)
            .append_pair("q", keyword.as_str())
            .append_pair("pos", "0")
            .append_pair("cnt", "100")
            .append_pair("is_file", "0")
            .append_pair("no_muxer", "1");
    }
    notice!("search uri {}", search_uri.as_str());
    match rt.block_on(
        client
            .get(search_uri.as_str().parse().unwrap())
            .and_then(|res| res.into_body().concat2())
            .from_err::<FetchError>()
            .and_then(|body| {
                let res: Res = serde_json::from_slice(&body)?;
                if res.sub.subs.len() > 0 {
                    Ok(res)
                } else {
                    Err(FetchError::Logic("no subtitles found".to_owned()))
                }
            })
            .from_err::<FetchError>()
            .and_then(move |res| {
                let mut index: usize;
                loop {
                    notice!(
                        "there are more subtitles found, please select one of them, input number:"
                    );
                    let mut i = 0;
                    for s in &res.sub.subs {
                        notice!("[{}]{:?}", i, s);
                        i += 1;
                    }
                    index = read!();
                    if index < res.sub.subs.len() {
                        break;
                    } else {
                        notice!("please input a valid id");
                    }
                }
                {
                    let mut query = detail_uri.query_pairs_mut();
                    query.append_pair("token", token);
                    query.append_pair("id", res.sub.subs[index].id.to_string().as_str());
                }
                notice!("detail url {:?}", detail_uri);
                client
                    .get(detail_uri.as_str().parse().unwrap())
                    .and_then(|res| res.into_body().concat2())
                    .from_err::<FetchError>()
                    .and_then(|body| {
                        notice!("detail ret {:?}", body);
                        let res: Res = serde_json::from_slice(&body)?;
                        if res.sub.subs.len() == 0 {
                            Err(FetchError::Logic("detail fetch failed".to_owned()))
                        } else {
                            Ok(res)
                        }
                    })
                    .and_then(|res| {
                        notice!("download url {:?}", res.sub.subs[0].url);
                        let client = Client::new();
                        client
                            .get(res.sub.subs[0].url.parse().unwrap())
                            .and_then(|res| res.into_body().concat2())
                            .from_err::<FetchError>()
                            .and_then(move |body| {
                                let buffer = std::io::Cursor::new(&body);
                                if let Ok(mut reader) = ZipArchive::new(buffer) {
                                    notice!("zip file found");
                                    let mut buffer: [u8; 1024] = [0; 1024];
                                    let mut files = Vec::new();
                                    for i in 0..reader.len() {
                                        let mut file = reader.by_index(i)?;
                                        let name = file.sanitized_name();
                                        if file.name().ends_with('/') {
                                            continue;
                                        }
                                        notice!("unzip file {:?} {}", name, file.size());
                                        let name = name.file_name().unwrap().to_str().unwrap();
                                        let mut out = std::fs::File::create(name)?;
                                        loop {
                                            let size = file.read(&mut buffer)?;
                                            if size == 0 {
                                                break;
                                            }
                                            out.write_all(&buffer[..size])?;
                                        }
                                        out.flush()?;
                                        files.push(name.to_owned());
                                    }
                                    Ok(files)
                                } else {
                                    let name = rand::thread_rng()
                                        .sample_iter(&rand::distributions::Alphanumeric)
                                        .take(30)
                                        .collect::<String>();
                                    let name = std::env::temp_dir().join(&name);
                                    let mut file = std::fs::File::create(&name)?;
                                    file.write_all(&body)?;
                                    file.flush()?;
                                    Err(FetchError::Logic(name.to_str().unwrap().to_owned()))
                                }
                            })
                    })
                    .or_else(|err| {
                        if let FetchError::Logic(name) = err {
                            notice!("rar file found {}", name);
                            let source = std::env::temp_dir()
                                .to_path_buf()
                                .to_str()
                                .unwrap()
                                .to_string();
                            let target = ".".to_string();
                            if let Ok(mut archive) = Archive::new(name).extract_to(source) {
                                if let Ok(entries) = archive.process() {
                                    let mut files = Vec::new();
                                    for entry in &entries {
                                        if entry.is_directory() {
                                            continue;
                                        }
                                        let from = std::env::temp_dir().join(&entry.filename);
                                        let to = Path::new(&target).join(from.file_name().unwrap());
                                        notice!("copy from {:?} to {:?}", &from, &to);
                                        std::fs::copy(&from, &to)?;
                                        files.push(
                                            from.file_name().unwrap().to_str().unwrap().to_string(),
                                        );
                                    }
                                    return Ok(files);
                                }
                            }
                            Err(FetchError::Logic("unrar failed".to_string()))
                        } else {
                            println!("save data failed {:?}", err);
                            Err(err)
                        }
                    })
                    .and_then(move |files| {
                        if files.len() == 0 {
                            Err(FetchError::Logic(
                                "no subtitles found in archive".to_owned(),
                            ))
                        } else if let Some(suffix_offset) = files[0].rfind(".") {
                            println!("suffix offset {}", suffix_offset);
                            let mut index = 0;
                            'outer: loop {
                                for i in 0..files.len() {
                                    if index >= suffix_offset - 1 {
                                        break 'outer;
                                    }
                                    if index >= files[i].len() {
                                        break 'outer;
                                    }
                                    if files[0].as_bytes()[index] != files[i].as_bytes()[index] {
                                        if index > 0 && files[0].as_bytes()[index - 1] == '.' as u8
                                        {
                                            index -= 1;
                                        }
                                        break 'outer;
                                    }
                                }
                                index += 1;
                            }
                            notice!(
                                "common prefix is {} {}",
                                index,
                                String::from_utf8_lossy(&files[0].as_bytes()[..index])
                            );
                            for file in &files {
                                let (_, suffix) = file.split_at(index);
                                let mut to = video_file.clone();
                                to.push_str(suffix);
                                std::fs::rename(&file, &to)?;
                                notice!("rename from {} to {}", file, to);
                            }
                            Ok(())
                        } else {
                            Err(FetchError::Logic(format!("invalid subtitles {}", files[0])))
                        }
                    })
            }),
    ) {
        Ok(data) => {
            notice!("fetch succeed with {:?}", data);
            true
        }
        Err(err) => {
            notice!("fetch failed with {:?}", err);
            false
        }
    }
}

fn main() {
    if std::env::args().len() < 2 {
        notice!("no video file name found");
        return;
    }
    let mut video_file = std::env::args().nth(1).unwrap();
    if let Some(i) = video_file.rfind(".") {
        video_file.split_off(i);
    } else {
        notice!("invalid video file specified");
        return;
    }
    let mut keyword = video_file.replace(".", " ").replace("+", " ");
    loop {
        if download(video_file.clone(), &keyword) {
            break;
        } else {
            if let Some(i) = keyword.rfind(" ") {
                keyword.split_off(i);
            } else {
                break;
            }
        }
    }
}
