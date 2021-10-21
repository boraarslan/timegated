use super::*;
pub async fn build_multipart_request(file_path: &str, mime_type: &str) -> Form {
    let contents = tokio::fs::read(file_path).await.unwrap();
    let form = multipart::Form::new().part(
        "",
        multipart::Part::bytes(contents)
            .mime_str(mime_type)
            .unwrap(),
    );
    form
}
