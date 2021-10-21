use std::convert::Infallible;

use axum::{
    extract::{
        multipart::MultipartRejection,
        rejection::{self, ContentLengthLimitRejection},
        ContentLengthLimit, Multipart, Path,
    },
    handler::{get, post},
    http::StatusCode,
    routing::BoxRoute,
    service, Router,
};
use mime;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tree_magic_mini;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "example_testing=debug,tower_http=debug")
    }
    tracing_subscriber::fmt::init();
    let app = app();
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn root_get_handler() -> String {
    "Hello World!".to_string()
}

async fn greet_user(user_id: Result<Path<u32>, rejection::PathParamsRejection>) -> String {
    match user_id {
        Ok(user_id) => {
            format!("Hello User {}!", user_id.0)
        }
        Err(rejection::PathParamsRejection::InvalidPathParam(_)) => {
            format!("Please provide a number within valid range.")
        }
        Err(rejection::PathParamsRejection::MissingRouteParams(_)) => {
            format!("Missing user id.")
        }
        Err(_) => {
            unreachable!()
        }
    }
}

async fn upload(
    multipart_body: Result<
        ContentLengthLimit<Multipart, { 25 * 1024 * 1024 }>,
        ContentLengthLimitRejection<MultipartRejection>,
    >,
) -> (StatusCode, String) {
    match multipart_body {
        Ok(multipart) => {
            let mut multipart = multipart.0;

            while let Some(field) = multipart.next_field().await.unwrap() {
                let content_type = field.content_type();

                if content_type == None {
                    return (StatusCode::BAD_REQUEST, format!("Content type is empty."));
                }
                if content_type.unwrap().type_() != mime::IMAGE {
                    return (
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        format!("Content must be image."),
                    );
                }

                let data = field.bytes().await.unwrap_or_default();
                let data_mime_type = tree_magic_mini::from_u8(&data);

                if !data_mime_type.starts_with("image") {
                    return (
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        format!("Content must be image."),
                    );
                }

                let file_name = Uuid::new_v4().to_hyphenated();

                if let Err(_) =
                    tokio::fs::write(format!("user_shots/{}.jpeg", file_name), data).await
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error writing to file."),
                    );
                }

                return (StatusCode::OK, format!("/img/{}.jpeg", file_name));
            }

            (StatusCode::OK, format!("Ok"))
        }
        Err(ContentLengthLimitRejection::PayloadTooLarge(_)) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("File size must be smaller than 25 MBs."),
        ),
        Err(ContentLengthLimitRejection::LengthRequired(_)) => (
            StatusCode::LENGTH_REQUIRED,
            format!("File length required."),
        ),
        Err(_) => (StatusCode::BAD_REQUEST, format!("Bad Request!")),
    }
}

fn app() -> Router<BoxRoute> {
    Router::new()
        .route("/upload", post(upload))
        .route("/user/:id", get(greet_user))
        .nest(
            "/img",
            service::get(ServeDir::new("user_shots")).handle_error(|error: std::io::Error| {
                Ok::<_, Infallible>((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Unhandled internal error: {}", error),
                ))
            }),
        )
        .route("/", get(root_get_handler))
        .layer(TraceLayer::new_for_http())
        .boxed()
}

#[cfg(test)]
mod tests;