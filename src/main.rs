mod entity;

use axum::{
    error_handling::HandleErrorExt,
    extract::{
        self, multipart::MultipartRejection, rejection::ContentLengthLimitRejection,
        ContentLengthLimit, Multipart,
    },
    http::StatusCode,
    routing::{post, service_method_routing},
    AddExtensionLayer, Router,
};
use chrono::{self, Duration, Utc};
use dotenv::dotenv;
use entity::*;
use sea_orm::{
    prelude::*, ActiveModelTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use std::{convert::Infallible, env};
use tokio::{spawn, time::interval};
use tokio_retry::{strategy::FixedInterval, Retry};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tree_magic_mini as check_mime_content;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    dotenv().ok();
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "example_testing=debug,tower_http=debug")
    }

    tracing_subscriber::fmt::init();

    let database_url =
        env::var("DATABASE_URL").unwrap_or("postgres://localhost:5432/postgres".to_string());
    let timegated_port = env::var("TIMEGATED_PORT").unwrap_or("3000".to_string());

    let db = Database::connect(database_url).await.unwrap();

    spawn(delete_scheduler(db.clone()));

    let app = app(db);
    axum::Server::bind(&format!("0.0.0.0:{}", timegated_port).parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn delete_scheduler(db: DatabaseConnection) {
    let mut interval = interval(tokio::time::Duration::from_secs(10));
    loop {
        interval.tick().await;
        let deadline = Utc::now().naive_utc() - Duration::seconds(43200);

        let timegated_photos: Vec<photo_data::Model> = photo_data::Entity::find()
            .filter(photo_data::Column::Timestamp.lt(deadline))
            .all(&db)
            .await
            .unwrap_or_default();
        println!("{:?}", timegated_photos);

        for to_delete in timegated_photos {
            let photo_data: photo_data::ActiveModel = to_delete.into();
            // even if it fails now it will probably will get deleted next iter.
            if Retry::spawn(FixedInterval::from_millis(10).take(3), || async {
                photo_data.clone().delete(&db).await
            })
            .await
            .is_err()
            {
                continue;
            }

            if Retry::spawn(FixedInterval::from_millis(10).take(3), || async {
                tokio::fs::remove_file(format!(
                    "user_shots/{}.jpeg",
                    photo_data.photo_id.clone().unwrap()
                ))
                .await
            })
            .await
            .is_err()
            {
                // if error occurs reinsert the entry
                let _ = photo_data.insert(&db).await;
            }
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

            if let Some(field) = multipart.next_field().await.unwrap() {
                let content_type = field.content_type();

                if content_type == None {
                    return (
                        StatusCode::BAD_REQUEST,
                        "Content type is empty.".to_string(),
                    );
                }
                if content_type.unwrap().type_() != mime::IMAGE {
                    return (
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "Content must be image.".to_string(),
                    );
                }

                let data = field.bytes().await.unwrap_or_default();
                let data_mime_type = check_mime_content::from_u8(&data);

                if !data_mime_type.starts_with("image") {
                    return (
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "Content must be image.".to_string(),
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

                if Retry::spawn(FixedInterval::from_millis(10).take(3), || async {
                    photo_model.clone().insert(&db).await
                })
                .await
                .is_err()
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Error connecting to database.".to_string(),
                    );
                }

                if Retry::spawn(FixedInterval::from_millis(10).take(3), || async {
                    tokio::fs::write(format!("user_shots/{}.jpeg", file_name), &data).await
                })
                .await
                .is_err()
                {
                    // This shouldn't fail. At least thats what i thought when i wrote this.
                    let premature_insert = photo_data::Entity::find_by_id(photo_uuid)
                        .one(&db)
                        .await
                        .unwrap();
                    // This shouldn't fail either since we (succesfully) inserted it just a moment ago.
                    let premature_insert: photo_data::ActiveModel =
                        premature_insert.unwrap().into();
                    // TODO! If this operation fails init the standart delete sequence. or smthng like that.
                    let _delete_result = premature_insert.delete(&db).await;
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Error writing to file.".to_string(),
                    );
                }

                return (StatusCode::OK, format!("/img/{}.jpeg", file_name));
            }

            (StatusCode::OK, "Ok".to_string())
        }
        Err(ContentLengthLimitRejection::PayloadTooLarge(_)) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "File size must be smaller than 25 MBs.".to_string(),
        ),
        Err(ContentLengthLimitRejection::LengthRequired(_)) => (
            StatusCode::LENGTH_REQUIRED,
            "File length required.".to_string(),
        ),
        Err(_) => (StatusCode::BAD_REQUEST, "Bad Request!".to_string()),
    }
}

fn app(db_connection: DatabaseConnection) -> Router {
    Router::new()
        .route("/upload", post(upload))
        .nest(
            "/img",
            service_method_routing::get(ServeDir::new("user_shots")).handle_error(
                |error: std::io::Error| {
                    Ok::<_, Infallible>((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Unhandled internal error: {}", error),
                    ))
                },
            ),
        )
        .layer(TraceLayer::new_for_http())
        .layer(AddExtensionLayer::new(db_connection))
}

#[cfg(test)]
mod tests;
