use super::*;
use helpers::build_multipart_request;
#[tokio::test]
async fn upload_valid_file() {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").unwrap_or("postgres://localhost:5432/postgres".to_string());
    let db = Database::connect(database_url).await.unwrap();
    let app = app(db);
    let listener = TcpListener::bind("0.0.0.0:0".parse::<SocketAddr>().unwrap()).unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::Server::from_tcp(listener)
            .unwrap()
            .serve(app.into_make_service())
            .await
            .unwrap()
    });

    let form = build_multipart_request("./test_assets/image.jpeg", "image/").await;
    let client = reqwest::Client::new();
    let res = client
        .post(format!("http://{}/upload", addr))
        .multipart(form)
        .send()
        .await
        .unwrap();

    let res_code = res.status();
    let res_body = res.text().await.unwrap();
    println!("{}", res_body);
    assert_eq!(
        StatusCode::OK,
        res_code,
        "Server returned: {}\nBody was: {}",
        res_code,
        res_body
    );
}
