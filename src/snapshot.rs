use anyhow::Result;
use chrono::{DateTime, SubsecRound, Utc};
use chrono_tz::Europe::Vienna;
use chrono_tz::Tz;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, sync::OnceLock};
use suppaftp::FtpStream;
use suppaftp::list::File;

static CRAN_HOST: &str = "cran.r-project.org:21";
static CRAN_ROOT: &str = "/incoming";
static CRAN_USER: &str = "anonymous";
static CRAN_PASSWORD: &str = "anonymous";

fn package_file_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.+)_(.+)\.tar\.gz$").unwrap())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Submission {
    request_time: DateTime<Utc>,
    folder: String,
    file_time: DateTime<Utc>,
    file_bytes: usize,
    pkg_name: String,
    pkg_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

    pub fn capture() -> Result<Snapshot> {
        capture_snapshot()
    }
}

fn create_entry(
    ftp_file: &File,
    folder: &str,
    request_time: &DateTime<Utc>,
    modified_time: &DateTime<Utc>,
) -> Option<Submission> {
    if !ftp_file.is_file() {
        return None;
    }

    package_file_regex()
        .captures(ftp_file.name())
        .map(|caps| Submission {
            request_time: request_time.to_owned(),
            folder: folder[(CRAN_ROOT.len() + 1).min(folder.len())..].to_owned(),
            file_time: *modified_time,
            file_bytes: ftp_file.size(),
            pkg_name: caps.get(1).map_or("[unknown]", |c| c.as_str()).to_owned(),
            pkg_version: caps.get(2).map_or("[unknown]", |c| c.as_str()).to_owned(),
        })
}

fn capture_snapshot() -> Result<Snapshot> {
    let mut ftp_stream = FtpStream::connect(CRAN_HOST)?;
    ftp_stream.login(CRAN_USER, CRAN_PASSWORD)?;

    let capture_time = Utc::now();

    let mut snap = Snapshot {
        capture_time: capture_time.round_subsecs(0),
        capture_duration: 0,
        submissions: Vec::new(),
    };

    let max_depth: u32 = 2;
    let mut folder_stack: Vec<(u32, String)> = vec![(0, CRAN_ROOT.to_owned())];

    while let Some((depth, ftp_path)) = folder_stack.pop() {
        let request_time: DateTime<Utc> = Utc::now().round_subsecs(0);
        for ftp_res in ftp_stream.list(Some(&ftp_path))? {
            let ftp_file = File::from_str(&ftp_res)?;
            if ftp_file.is_directory() {
                if depth < max_depth {
                    folder_stack.push((depth + 1, [&ftp_path, ftp_file.name()].join("/")));
                }
            } else if ftp_file.is_file() {
                let local_time_result = ftp_stream
                    .mdtm([&ftp_path, ftp_file.name()].join("/"))
                    .unwrap_or(Utc::now().naive_utc())
                    .and_local_timezone::<Tz>(Vienna);

                // Daylight savings time bugfix
                let modified_time: DateTime<Utc> = match local_time_result {
                    chrono::LocalResult::None => Utc::now(),
                    chrono::LocalResult::Single(t) => t.with_timezone(&Utc),
                    chrono::LocalResult::Ambiguous(t, _) => t.with_timezone(&Utc),
                };

                if let Some(entry) =
                    create_entry(&ftp_file, &ftp_path, &request_time, &modified_time)
                {
                    snap.submissions.push(entry);
                }
            }
            // do nothing for symlinks
        }
    }

    snap.capture_duration = Utc::now()
        .signed_duration_since(capture_time)
        .num_milliseconds();

    Ok(snap)
}
