# kosync-proxy

A self-hosted [KOReader](https://github.com/koreader/koreader) sync server, written in Rust. It implements the kosync
protocol KOReader uses to keep your reading position in step across devices, stores everything in SQLite, and can
optionally forward every push to another kosync server.

- Drop-in custom sync server for KOReader (register, authenticate, push and pull progress)
- Credentials are stored as a bcrypt hash of the key the client sends, never the key itself
- Progress is kept as append-only history, so nothing is overwritten
- Optional forwarding of pushes to a second kosync server (for example the official one)
- Single static binary or a small Docker image

## Quick start

### Docker

```sh
docker run -d --name kosync \
  -p 3000:3000 \
  -v kosync-data:/data \
  ghcr.io/jordansekky/kosync-rs
```

The database lives in `/data`, so mount a volume there to keep it across restarts.

### From source

```sh
cargo run --release
```

This listens on `0.0.0.0:3000` and creates `kosync.db` in the working directory. Migrations run on startup.

### Pointing KOReader at it

In KOReader's progress sync settings, set the custom sync server to your server's base URL (for example
`http://192.168.1.10:3000`), then register or log in. The client appends the API paths itself, so a path prefix on the
URL works if you put the server behind a reverse proxy.

## Configuration

Every option can be given as a flag or an environment variable; a flag wins over the variable.

| Flag                    | Environment variable  | Default              | Description                                              |
| ----------------------- | --------------------- | -------------------- | -------------------------------------------------------- |
| `--database-url`        | `DATABASE_URL`        | `sqlite://kosync.db` | SQLite database to store users and progress in           |
| `--bind-addr`           | `BIND_ADDR`           | `0.0.0.0:3000`       | Address to listen on                                     |
| `--enable-registration` | `ENABLE_REGISTRATION` | `true`               | Whether new users can register (`true` or `false`)       |
| `--upstream-url`        | `UPSTREAM_URL`        | none                 | Base URL of another kosync server to forward pushes to   |

Logging is controlled with `RUST_LOG`, for example `RUST_LOG=kosync_proxy=debug`. The default level is `info`.

### Closing registration

Once your accounts exist you can turn registration off. Existing users keep working; only `POST /users/create` is
refused.

```sh
kosync-proxy --enable-registration=false
```

## Forwarding to another server

With `--upstream-url` set, every successful push is replayed against that server with the same credentials the client
sent, so both servers end up with the same position.

- The forward runs in the background; the client gets its response as soon as the local write is done.
- It is best-effort: a failure is logged as a warning and does not fail the client's push.
- Only pushes are forwarded. Pulls and registration are answered locally.
- On shutdown the server waits for forwards that are still in flight (each has a 5 second timeout).

## API

All requests and responses are JSON. Every endpoint except registration needs the headers `x-auth-user` and
`x-auth-key`, where the key is `md5(password)` computed by the client.

| Method | Path                         | Description                      | Success response                                          |
| ------ | ---------------------------- | -------------------------------- | --------------------------------------------------------- |
| `POST` | `/users/create`              | Register `{username, password}`  | `201` `{"username": "..."}`                               |
| `GET`  | `/users/auth`                | Check credentials                | `{"authorized": "OK"}`                                    |
| `PUT`  | `/syncs/progress`            | Push progress for a document     | `{"document": "...", "timestamp": 1700000000}`            |
| `GET`  | `/syncs/progress/{document}` | Pull the latest progress         | the record, or `{}` if the document was never synced      |

A push body looks like this (`device_id` and `metadata` are optional):

```json
{
  "document": "8b03a82761fae0ee6cd5a23700361e74",
  "progress": "/body/DocFragment[15]/body/div[65]/text()[1].41",
  "percentage": 0.2082,
  "device": "boox",
  "device_id": "197E7C6B3FD54A749C87DE9C1B05A3CE"
}
```

Errors come back as `{"message": "..."}` with the status codes below.

| Status | Meaning                                                      |
| ------ | ------------------------------------------------------------ |
| `400`  | Malformed JSON or an invalid path parameter                  |
| `401`  | Missing or wrong credentials (unknown users look the same)   |
| `403`  | Registration is disabled                                     |
| `404`  | Unknown route                                                |
| `405`  | Known route, wrong method                                    |
| `409`  | The username is already registered                           |
| `415`  | Body sent without `Content-Type: application/json`           |
| `422`  | Valid JSON with a missing field or a wrong type              |
| `502`  | An upstream request failed                                   |
| `500`  | Internal error                                               |

A pull for a document that was never synced answers `200 {}` rather than a `404`, matching the reference server; the
client reads a missing `percentage` as "no progress yet".

## How it works

- **Storage**: SQLite in WAL mode with two connection pools, a single-connection pool for writes and a read-only pool
  sized to the CPU count.
- **History**: every push appends a row to `progress_history`. The `latest_progress` and `latest_device_progress` views
  give the newest entry per document and per device. Timestamps are stored as unix nanoseconds and reported to clients in
  seconds.
- **Authentication**: the key the client sends is bcrypt-hashed before it is stored. Verification runs on a blocking
  thread, and an unknown username does the same amount of work as a wrong key so account existence is not leaked.
- **Shutdown**: on ctrl-c or SIGTERM the server stops accepting connections, lets in-flight requests and forwards
  finish, then closes the database pools so SQLite can checkpoint and clean up its WAL files.

## Building

```sh
cargo build --release
```

The Docker image is built with [cargo-chef](https://github.com/LukeMathWalker/cargo-chef) against musl, giving a static
binary. A GitHub Actions workflow in `.github/workflows/docker-publish.yml` builds the image on pushes and pull
requests to `main` and publishes it to `ghcr.io` on pushes.

## Known limitations

- Progress history is never pruned.
- There is no health-check endpoint and no rate limiting.
