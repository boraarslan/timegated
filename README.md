# Timegated

This project is an async web server written with [Axum](https://github.com/tokio-rs/axum) that hosts uploaded images for 12 hours then deletes after it.

To run clone the repository, download [Rust](https://www.rust-lang.org/tools/install), download [PostgreSQL](https://www.postgresql.org/download/) and run:

```bash
cargo run
```

# Dependencies
- Rust
- PostgreSQL

Web server connects to PostgreSQL database using the `DATABASE_URL` and binds to `TIMEGATED_PORT` variables in `.env` file.

# Features

Server hosts images on `/img/*.jpeg`. Server accepts multipart request from `/upload` endpoint.

>- Max 25MB is allowed.
>- All images are type checked.

If upload is successful server returns the endpoint the image is hosted at.