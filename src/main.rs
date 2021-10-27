mod entity;

use axum::{
    extract::{
        self,
        multipart::MultipartRejection,
        rejection::{self, ContentLengthLimitRejection},
        ContentLengthLimit, Multipart, Path,
    },
    handler::{get, post},
    http::StatusCode,
    routing::BoxRoute,
    service, AddExtensionLayer, Router,
};
use chrono::{self, Utc};
use dotenv::dotenv;
use entity::*;
use mime;
use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, EntityTrait, Set};
use std::{convert::Infallible, env};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tree_magic_mini as check_mime_content;
use uuid::Uuid;

// TODO! Write an async deleter function that will run with the server
// and make db call for every 10 seconds for the photos that will be
// deleted.

#[tokio::main]
async fn main() {
    dotenv().ok();
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "example_testing=debug,tower_http=debug")
    }

    tracing_subscriber::fmt::init();

    let db = Database::connect(env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = app(db);
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
    db_connection: extract::Extension<DatabaseConnection>,
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
                let data_mime_type = check_mime_content::from_u8(&data);

                if !data_mime_type.starts_with("image") {
                    return (
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        format!("Content must be image."),
                    );
                }

                let photo_uuid = Uuid::new_v4();
                let file_name = photo_uuid.to_hyphenated();

                let current_time_utc = Utc::now().naive_utc();
                let photo_model = photo_data::ActiveModel {
                    photo_id: Set(photo_uuid),
                    timestamp: Set(current_time_utc),
                };

                let db = db_connection.0;

                if let Err(_) = photo_model.insert(&db).await {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error connecting to database."),
                    );
                }

                if let Err(_) =
                    tokio::fs::write(format!("user_shots/{}.jpeg", file_name), data).await
                {
                    // This shouldn't fail. At least thats what i thought when i wrote this. 
                    let premature_insert = photo_data::Entity::find_by_id(photo_uuid).one(&db).await.unwrap();
                    // This shouldn't fail either since we (succesfully) inserted it just a moment ago. 
                    let premature_insert: photo_data::ActiveModel = premature_insert.unwrap().into();
                    // TODO! If this operation fails init the standart delete sequence. or smthng like that.
                    let _delete_result = premature_insert.delete(&db).await;
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

fn app(db_connection: DatabaseConnection) -> Router<BoxRoute> {
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
        .layer(AddExtensionLayer::new(db_connection))
        .boxed()
}

#[cfg(test)]
mod tests;
