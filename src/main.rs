#[macro_use]
extern crate rocket;
mod snapshot;
use rocket::{
    serde::{Deserialize, Serialize, json},
    tokio::sync::{Mutex, RwLock},
    State, fairing::{Fairing, Info, Kind}, Request, Response, http::Header, Config,
};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH}, net::Ipv4Addr,
};

static TIMEOUT_CACHE_SECONDS: u64 = 60*10;


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
struct SnapshotContainer {
    update_interval: u64,
    snapshot: snapshot::Snapshot,
}

pub struct Cache {
    last_update: Arc<Mutex<SystemTime>>,
    data: Arc<RwLock<SnapshotContainer>>,
}

pub struct CORS;

#[rocket::async_trait]
impl Fairing for CORS {
    fn info(&self) -> Info {
        Info {
            name: "Add CORS headers to responses",
            kind: Kind::Response
        }
    }

    async fn on_response<'r>(&self, _request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new("Access-Control-Allow-Methods", "POST, GET, PATCH, OPTIONS"));
        response.set_header(Header::new("Access-Control-Allow-Headers", "*"));
        response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));
    }
}

#[get("/")]
fn index() -> &'static str {
    "Hello, CRAN!"
}

#[get("/snap")]
async fn snap(cache: &State<Cache>) -> json::Json<SnapshotContainer> {
    {
        let mut last_update = cache.last_update.lock().await;

        let now = SystemTime::now();

        if now
            .duration_since(*last_update)
            .expect("Time went backwards")
            .as_secs()
            > TIMEOUT_CACHE_SECONDS
        {
            println!("Update cache");
            *last_update = now;
            let mut x = cache.data.write().await;
            match snapshot::Snapshot::capture() {
                Ok(snap) => x.snapshot = snap,
                Err(err) => println!("ERROR: Could not create snapshot: {}", err),
            }
        } else {
            println!("Use cached");
        }
    }

    json::Json(cache.data.read().await.clone())
}

#[launch]
fn rocket() -> _ {
    let config = Config {
        port: 8080,
        address: Ipv4Addr::new(0, 0, 0, 0).into(),
        ..Config::debug_default()
    };

    rocket::build()
        .configure(config)
        .attach(CORS)
        .manage(Cache {
            last_update: Arc::new(Mutex::new(UNIX_EPOCH)),
            data: Arc::new(RwLock::new(SnapshotContainer {
                update_interval: TIMEOUT_CACHE_SECONDS,
                snapshot: snapshot::Snapshot::new(),
            })),
        })
        .mount("/", routes![index, snap])
}
