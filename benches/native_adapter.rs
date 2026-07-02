use comet::{WorkerBody, WorkerRequest};
use criterion::{Criterion, criterion_group, criterion_main};
use rocket::serde::{Deserialize, Serialize, json::Json};

#[macro_use]
extern crate rocket;

#[derive(Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
struct Echo<'a> {
    message: &'a str,
}

#[get("/")]
fn index() -> &'static str {
    "hello from Rocket on comet"
}

#[post("/echo", format = "json", data = "<body>")]
fn echo<'a>(body: Json<Echo<'a>>) -> Json<Echo<'a>> {
    body
}

async fn worker() -> comet::RocketWorker {
    comet::RocketWorker::new(rocket::build().mount("/", routes![index, echo]))
        .await
        .expect("rocket client")
}

fn native_adapter_benches(c: &mut Criterion) {
    let runtime = rocket::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let worker = runtime.block_on(worker());

    c.bench_function("native_adapter_get_small", |b| {
        b.iter(|| {
            let response = runtime
                .block_on(worker.dispatch(WorkerRequest::get("/")))
                .expect("dispatch");
            assert_eq!(response.status, 200);
            assert!(matches!(response.body, WorkerBody::Buffered(_)));
        });
    });

    c.bench_function("native_adapter_post_json", |b| {
        b.iter(|| {
            let response = runtime
                .block_on(
                    worker.dispatch(
                        WorkerRequest::post("/echo", br#"{"message":"hello"}"#)
                            .header("content-type", "application/json"),
                    ),
                )
                .expect("dispatch");
            assert_eq!(response.status, 200);
            assert!(matches!(response.body, WorkerBody::Buffered(_)));
        });
    });
}

criterion_group!(benches, native_adapter_benches);
criterion_main!(benches);
