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

The database lives in `/data`, so mount a volume there to keep it across restarts. The [admin listener](#admin-listener)
is not published here: it listens on loopback inside the container by default, so to reach it from the host also set
`-e ADMIN_BIND_ADDR=0.0.0.0:3001` and publish `-p 127.0.0.1:3001:3001`.

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
| `--bind-addr`           | `BIND_ADDR`           | `0.0.0.0:3000`       | Address the sync API listens on                          |
| `--admin-bind-addr`     | `ADMIN_BIND_ADDR`     | `127.0.0.1:3001`     | Address the admin listener (health check and admin API)  |
| `--enable-registration` | `ENABLE_REGISTRATION` | `true`               | Whether new users can register (`true` or `false`)       |
| `--upstream-url`        | `UPSTREAM_URL`        | none                 | Base URL of another kosync server to forward pushes to   |
| `--admin-path-prefix`   | `ADMIN_PATH_PREFIX`   | none                 | Path the admin routes are served under, e.g. `/api`      |
| `--admin-cors-origins`  | `ADMIN_CORS_ORIGINS`  | none                 | Origins browsers may call the admin API from (see below) |
| `--log-buffer-lines`    | `LOG_BUFFER_LINES`    | `1000`               | Log lines kept in memory for the admin log stream (`0` keeps none) |
| `--print-openapi`       |                       | off                  | Print the admin API's OpenAPI document and exit          |

Logging is controlled with `RUST_LOG`, for example `RUST_LOG=kosync_proxy=debug`. The default level is `info`. The admin
API's [log stream](#log-stream) carries the same lines that go to stdout.

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

### Admin listener

A second listener, on its own port, serves a read-only API for inspecting users and their progress history. It is meant
for a web UI and shares the shutdown behaviour of the main listener.

**It has no authentication.** It listens on `127.0.0.1` by default; if you bind it elsewhere, put it behind something
that authenticates.

Which web pages a browser lets call it is set with `--admin-cors-origins`:

| Value                                             | Effect                                                              |
| ------------------------------------------------- | ------------------------------------------------------------------- |
| empty (the default)                               | None: only same-origin pages and non-browser clients can use it     |
| `http://localhost:5173,https://admin.example.com` | Only these origins, written as `scheme://host[:port]` with no path  |
| `*`                                               | Any origin. Any page the browser loads can read the API.            |

A web client served from a different origin than the API, such as a dev server on another port, has to be listed here.

CORS is enforced by the browser, so it narrows who can read the API from a web page; it does not stop `curl` or anything
else that can reach the port.

The paths below are relative to `--admin-path-prefix`.

| Method | Path          | Description                                                        | Response                |
| ------ | ------------- | ------------------------------------------------------------------ | ----------------------- |
| `GET`  | `/health`     | Liveness check                                                     | `{"status": "ok"}`      |
| `GET`  | `/users`      | Registered users, with entry and book counts and last activity     | `{"users": [...]}`      |
| `GET`  | `/users/{id}` | One user, same shape                                               | user, or `404`          |
| `GET`  | `/progress`   | Progress history across all users, newest first                    | `{"entries", "next_cursor"}` |
| `GET`  | `/books`      | Documents with history, for populating a book filter               | `{"books": [...]}`      |
| `GET`  | `/devices`    | Devices progress was pushed from, for populating a device filter   | `{"devices": [...]}`    |
| `GET`  | `/logs`       | Application log, cached lines first and then live ([details](#log-stream)) | `text/event-stream` |
| `GET`  | `/api.json`   | OpenAPI 3.1 document describing all of the above                   | OpenAPI JSON            |

`/progress` takes these query parameters, all optional and combined with AND:

| Parameter             | Meaning                                                                     |
| --------------------- | --------------------------------------------------------------------------- |
| `user_id`             | Only this user's entries                                                    |
| `document`            | Only this document, as listed by `/books`                                   |
| `device`, `device_id` | Only entries pushed from a device with this name / id                       |
| `from`, `to`          | RFC 3339 times; `from` is inclusive and `to` exclusive                      |
| `limit`               | Page size, 100 by default and at most 500                                   |
| `cursor`              | The previous page's `next_cursor`; absent on the first page                 |

`next_cursor` is `null` on the last page. Paging is keyed on the entry id rather than an offset, so pages stay stable
while new pushes arrive. `/books` and `/devices` accept `user_id` to narrow the list to one user.

Timestamps are RFC 3339 strings with nanosecond precision. Errors have the same `{"message": "..."}` shape as the sync
API, and `title`, `authors` and `filename` are `null` for documents whose client never sent metadata.

#### Log stream

`GET /logs` is a [server-sent events](https://developer.mozilla.org/docs/Web/API/Server-sent_events) stream of what the
application logs. A new connection first receives the cached lines, oldest first, and then each new line as it is
logged. The cache holds the last `--log-buffer-lines` lines and lives in memory, so it starts empty after a restart.

```js
const source = new EventSource("/api/logs");
source.onmessage = (event) => {
  const entry = JSON.parse(event.data);
  console.log(entry.timestamp, entry.level, entry.message, entry.fields);
};
```

Each event's `data` is a JSON object:

| Field       | Meaning                                                                                  |
| ----------- | ---------------------------------------------------------------------------------------- |
| `id`        | Counts up by one per line, starting at 1 on each start of the server                     |
| `timestamp` | RFC 3339 time the line was logged                                                        |
| `level`     | `trace`, `debug`, `info`, `warn` or `error`                                              |
| `target`    | The module that logged it, such as `kosync_proxy::upstream`                              |
| `message`   | The log message, empty if there was none                                                 |
| `fields`    | The other fields logged with it, as strings                                              |
| `spans`     | The spans it was logged inside of, outermost first, each `{name, fields}`                |

- **Reconnecting**: `EventSource` reconnects by itself and sends `Last-Event-ID`, and the stream then continues after
  that line instead of replaying the cache. An id from before the server restarted is ignored, so the whole cache is
  sent.
- **Falling behind**: a client that can't keep up skips ahead rather than slowing the server down. A jump in `id`
  shows where lines were dropped.
- **Levels**: the stream only has what passes `RUST_LOG`, so raise the level to see more in it.
- **Reverse proxies**: the response sets `X-Accel-Buffering: no` for nginx. Other proxies that buffer responses need
  that turned off for this path.
- **Privacy**: the log includes usernames and upstream errors, and like the rest of the admin API it has no
  authentication.
- **Shutdown**: open streams are closed when the server shuts down, so a page left open doesn't hold it up.

#### Sharing a port with a web client

With `--admin-path-prefix /api`, every admin route, `/health` and `/api.json` included, is served under `/api`
(`/api/users`, `/api/health`, ...) and anything outside of it is a `404`. A reverse proxy can then send `/api/` to the
admin listener and everything else to wherever the web client is served from, on one host and port. The proxy has to
forward the path unchanged: do not strip the prefix. In Caddy that is `handle`, not `handle_path`:

```
example.com {
    handle /api/* {
        reverse_proxy 127.0.0.1:3001
    }
    handle {
        root * /srv/kosync-web
        try_files {path} /index.html
        file_server
    }
}
```

The browser then sees one origin, so no CORS origins need to be configured. The prefix does not apply to the sync API on
the main listener, and it is only a path: it does not add authentication.

The OpenAPI document keeps the routes' own paths (`/users`, not `/api/users`) and lists the prefix as its `servers` entry,
so a generated client is the same for every deployment and takes the prefix as its base URL.

To generate a client, point a generator at `/api.json`, or write the document without starting a server:

```sh
kosync-proxy --print-openapi > openapi.json
npx openapi-typescript openapi.json -o api.d.ts
```

## How it works

- **Storage**: SQLite in WAL mode with two connection pools, a single-connection pool for writes and a read-only pool
  sized to the CPU count.
- **History**: every push appends a row to `progress_history`. The `latest_progress` and `latest_device_progress` views
  give the newest entry per document and per device. Timestamps are stored as unix nanoseconds and reported to clients in
  seconds.
- **Authentication**: the key the client sends is bcrypt-hashed before it is stored. Verification runs on a blocking
  thread, and an unknown username does the same amount of work as a wrong key so account existence is not leaked.
- **Shutdown**: on ctrl-c or SIGTERM both listeners stop accepting connections, lets in-flight requests and forwards
  finish (open log streams are ended so they don't hold this up), then closes the database pools so SQLite can
  checkpoint and clean up its WAL files.

## Building

```sh
cargo build --release
```

The Docker image is built with [cargo-chef](https://github.com/LukeMathWalker/cargo-chef) against musl, giving a static
binary. A GitHub Actions workflow in `.github/workflows/docker-publish.yml` builds the image on pushes and pull
requests to `main` and publishes it to `ghcr.io` on pushes.

## Known limitations

- Progress history is never pruned.
- The log stream's cache is in memory only and is lost on restart.
- The health check only reports that the server is accepting requests; it does not touch the database.
- The admin API is unauthenticated and read-only, and rejects cross-origin browser requests unless `--admin-cors-origins` lists the origin.
- There is no rate limiting.
