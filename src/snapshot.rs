use chrono::{DateTime, SubsecRound, Utc};
use chrono_tz::Europe::Vienna;
use chrono_tz::Tz;
use lazy_static::lazy_static;
use regex::Regex;
use std::{error, str::FromStr};
use suppaftp::list::File;
use suppaftp::FtpStream;

use rocket::serde::{Deserialize, Serialize};

static CRAN_HOST: &'static str = "cran.r-project.org:21";
static CRAN_ROOT: &'static str = "/incoming";
static CRAN_USER: &'static str = "anonymous";
static CRAN_PASSWORD: &'static str = "anonymous";

lazy_static! {
    static ref RE_PACKAGE_FILE: Regex = Regex::new(r"^(.+)_(.+)\.tar\.gz$").unwrap();
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
pub struct Submission {
    request_time: DateTime<Utc>,
    folder: String,
    //file_name: String,
    file_time: DateTime<Utc>,
    file_bytes: usize,
    pkg_name: String,
    pkg_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
pub struct Snapshot {
    capture_time: DateTime<Utc>,
    capture_duration: i64,
    submissions: Vec<Submission>,
}

impl Snapshot {
    pub fn new() -> Snapshot {
        Snapshot {
            capture_time: Utc::now(),
            capture_duration: 0,
            submissions: Vec::new(),
        }
    }

    pub fn capture() -> Result<Snapshot, Box<dyn error::Error>> {
        capture_snapshot()
    }
}

fn create_entry(ftp_file: &File, folder: &str, request_time: &DateTime<Utc>, modified_time: &DateTime<Utc>) -> Option<Submission> {
    if !ftp_file.is_file() {
        return None;
    }

    match RE_PACKAGE_FILE.captures(ftp_file.name()) {
        Some(caps) => {
            Some(Submission {
                request_time: request_time.to_owned(),
                folder: folder[(CRAN_ROOT.len() + 1).min(folder.len())..].to_owned(),
                //file_name: ftpfile_sub.name().to_owned(),
                file_time: modified_time.clone(),
                file_bytes: ftp_file.size(),
                pkg_name: caps.get(1).map_or("[unknown]", |c| c.as_str()).to_owned(),
                pkg_version: caps.get(2).map_or("[unknown]", |c| c.as_str()).to_owned(),
            })
        }
        None => None,
    }
}



fn capture_snapshot() -> Result<Snapshot, Box<dyn error::Error>> {
    // create connection
    let mut ftp_stream = FtpStream::connect(CRAN_HOST)?;
    let _ = ftp_stream.login(CRAN_USER, CRAN_PASSWORD)?;

    let capture_time = Utc::now();

    let mut snap = Snapshot {
        capture_time: capture_time.round_subsecs(0),
        capture_duration: 0,
        submissions: Vec::new(),
    };

    // recursively traverse folders

    let max_depth: u32 = 2;
    let mut folder_stack: Vec<(u32, String)> = vec![(0, CRAN_ROOT.to_owned())];

    while let Some((depth, ftp_path)) = folder_stack.pop() {
        //println!("Explore depth {}: '{}'", depth, ftp_path);

        let request_time: DateTime<Utc> = Utc::now().round_subsecs(0);
        for ftp_res in ftp_stream.list(Some(&ftp_path))? {
            let ftp_file = File::from_str(&ftp_res)?;
            if ftp_file.is_directory() {
                if depth < max_depth {
                    folder_stack.push((depth + 1, [&ftp_path, ftp_file.name()].join("/")));
                }
            } else if ftp_file.is_file() {
                let modified_time: DateTime<Utc> = ftp_stream
                    .mdtm([&ftp_path, ftp_file.name()].join("/"))
                    .unwrap_or(Utc::now().naive_utc())
                    .and_local_timezone::<Tz>(Vienna)
                    .unwrap()
                    .with_timezone(&Utc);
                if let Some(entry) = create_entry(&ftp_file, &ftp_path, &request_time, &modified_time) {
                    snap.submissions.push(entry);
                }
            }
            // do nothing for symlinks
        }
    }

    snap.capture_duration = Utc::now()
        .signed_duration_since(capture_time)
        .num_milliseconds();

    return Ok(snap);
}
