# Using the admin API from a generated TypeScript client

The admin API describes itself with an OpenAPI document, so a typed client for it can be generated rather than written
by hand. This guide covers generating one with [`@hey-api/openapi-ts`](https://heyapi.dev/) and the behaviour you need
to know about when using it, most of all for the log stream.

It was written against `@hey-api/openapi-ts` 0.99.0 with its `@hey-api/client-fetch` client, Node 26 and TypeScript 5.9,
and every behaviour described below was observed against a running server. The SSE client is still changing quickly, so
pin the version you generate with and re-check this guide when you upgrade it.

The admin API is read-only and **not authenticated**. Keep it on loopback or behind a reverse proxy that authenticates
(see the [admin listener](../README.md#admin-listener) section of the README).

## Generating the client

You need Node 22.18 or later and TypeScript 5.5 or later. TypeScript 7 does not work: the generator needs the
JavaScript compiler API and fails on startup without it.

Get the spec from a binary, which needs no database or open port:

```sh
kosync-proxy --print-openapi > openapi.json
npx @hey-api/openapi-ts -i openapi.json -o src/client -c @hey-api/client-fetch
```

or from a running server, which serves it at `/api.json` (under the path prefix, if one is set):

```sh
npx @hey-api/openapi-ts -i http://127.0.0.1:3001/api.json -o src/client -c @hey-api/client-fetch
```

The same options as an `openapi-ts.config.ts`:

```ts
import { defineConfig } from '@hey-api/openapi-ts';

export default defineConfig({
  input: 'openapi.json',
  output: 'src/client',
  plugins: ['@hey-api/client-fetch'],
});
```

Notes:

- The generated files are all marked `*.gen.ts`. Don't edit them; regenerate when the server changes. The spec's
  version is the server's version.
- The generated code uses `URLSearchParams.entries()`, so your `tsconfig.json` needs `"DOM.Iterable"` in `lib`.

## Pointing it at the server

The spec names the server's path prefix as its server, or `/` when there is none, and the generated default `baseUrl`
follows it:

| Server started with                | Generated default `baseUrl`             |
| ---------------------------------- | --------------------------------------- |
| no path prefix                     | `'/'`                                   |
| `--admin-path-prefix /admin/api`   | `'/admin/api'`                          |

Both are relative, so they resolve against the origin of the page. That is right for a page served from the same
origin as the admin API, such as behind a reverse proxy. Otherwise, and always outside a browser (`fetch` in Node
rejects a relative URL), set the base URL yourself. The index file doesn't export the client instance, so import it
from `client.gen`:

```ts
import { client } from './client/client.gen';

client.setConfig({ baseUrl: 'http://127.0.0.1:3001' });
```

If the server runs with a path prefix, include it: `http://127.0.0.1:3001/admin/api`. The routes in the spec are
written without the prefix, so the generated functions never contain it.

A page served from a different origin than the admin API needs that origin allowed with `--admin-cors-origins`
(`ADMIN_CORS_ORIGINS`), for example `http://localhost:5173`. It defaults to none, so only same-origin pages and
non-browser clients can call the API.

## Calling endpoints

| Function       | Route             | Returns                                         |
| -------------- | ----------------- | ----------------------------------------------- |
| `health`       | `GET /health`     | `{ status: "ok" }`                              |
| `listUsers`    | `GET /users`      | `{ users }`, oldest account first               |
| `getUser`      | `GET /users/{id}` | a user; 404 if there is none                    |
| `listProgress` | `GET /progress`   | `{ entries, next_cursor }`, newest first, paged |
| `listBooks`    | `GET /books`      | `{ books }`, most recently synced first         |
| `listDevices`  | `GET /devices`    | `{ devices }`, most recently seen first         |
| `streamLogs`   | `GET /logs`       | a stream of `LogEntry`, see [below](#streaming-logs) |

`listProgress` takes the filters `user_id`, `document`, `device`, `device_id`, `from` (inclusive) and `to` (exclusive),
as well as `cursor` and `limit`. `listBooks` and `listDevices` take `user_id`. Parameters go in `query` and `path`:

```ts
const { data } = await listProgress({ query: { user_id: 1, document: 'abc' } });
```

### Results and errors

Every function resolves to `{ data, error, request, response }`, with exactly one of `data` and `error` set. Checking
`error` narrows `data`:

```ts
import { getUser } from './client';

export async function showUser(id: number) {
  const { data, error, response } = await getUser({ path: { id } });

  if (error) {
    // `response` is undefined when the request never got an answer
    const status = response?.status ?? 'network error';
    throw new Error(`${status}: ${error.message}`);
  }

  return data; // UserResponse
}
```

What `error` holds. It is typed `ErrorResponse`, but at runtime it can be one of three things, because the generated
client routes every failure through the same place:

- **The server answered with an error.** The body is `{ "message": string }`: `404` `Not Found.` for an unknown user
  or route, `400` with the parser's explanation for an id that isn't an integer or a query value of the wrong type,
  `500` `Internal Server Error.`. Every response from this server has that shape.
- **Something else answered with a body that isn't JSON**, such as a reverse proxy's HTML `502` page. `error` is then
  that body, **a string**, and `response.status` is the proxy's status.
- **The request never got an answer** (connection refused, DNS, CORS). The promise does **not** reject. It resolves
  with `error` set to the `TypeError` from `fetch` and `response` undefined.

So don't read `error.message` unchecked, and use `response` to tell "the server said no" from "nothing answered".
Rather than handling the three shapes everywhere, normalize them once with an error interceptor, after which `error`
really is `{ message: string }` in every case:

```ts
import { client } from './client/client.gen';

client.interceptors.error.use((error) => {
  if (error instanceof Error) return { message: error.message };
  if (typeof error === 'string') return { message: error.slice(0, 200) };
  return error;
});
```

The examples on this page assume it is installed.

To get exceptions instead, pass `throwOnError: true`. `data` is then never undefined. What is thrown is the same value
`error` would have held, so for a server error it is the **body, a plain object and not an `Error`**, with no stack
and `instanceof Error` false (an interceptor applies here too):

```ts
import { getUser, type ErrorResponse } from './client';

try {
  const { data } = await getUser({ path: { id }, throwOnError: true });
  return data;
} catch (e) {
  const { message } = e as ErrorResponse;
  throw new Error(message);
}
```

Without the interceptor, a network failure with `throwOnError: true` is thrown as the `TypeError` from `fetch`, which
*is* an `Error`, so the `catch` above still reads a `message` from it.

### Conventions in the types

- **Timestamps are ISO 8601 strings**, typed `string`, with up to nanosecond precision:
  `"2026-10-13T19:43:52.311112191Z"`. `new Date(...)` accepts them and drops the digits beyond milliseconds (checked in
  Node). Query parameters `from` and `to` take the same format: `new Date().toISOString()`.
- **Optional and nullable fields are `field?: T | null`.** Metadata the client never sent (`title`, `authors`,
  `filename`, `device_id`) and `last_active_at` for a user who never synced come back `null`. Test with `== null` or
  use `??`.
- **Ids are `number`.** Database ids are 64-bit, which a JavaScript number only holds exactly up to 2^53; that is far
  beyond what a self-hosted server will reach.
- **A document's id is a `string`** (a hash the KOReader client derives from the file), shared by every user who has
  the same book. Use `listBooks` to find the documents that exist and their titles:

  ```ts
  const { data } = await listBooks();
  const labels = (data?.books ?? []).map((book) => book.title ?? book.filename ?? book.document);
  ```

- **`*Response2` types.** Where a body type and the union of an operation's responses would have the same name, the
  generator suffixes the union: `ListUsersResponse` is the body, `ListUsersResponse2` is the same type reached through
  the operation. Use the one without the `2`.

## Pagination

Only `listProgress` is paged. Users, books and devices come back whole.

`limit` defaults to 100 and is clamped to 1 to 500. A response's `next_cursor` is the id of its last entry, or `null` on
the last page; pass it back as `cursor`. Entries are ordered by id, so pages don't shift when new entries arrive while
you are paging.

```ts
import { listProgress, type ListProgressQuery, type ProgressEntryResponse } from './client';

export async function* allProgress(
  filter: Omit<ListProgressQuery, 'cursor' | 'limit'> = {},
): AsyncGenerator<ProgressEntryResponse> {
  let cursor: number | null | undefined;

  do {
    const { data, error } = await listProgress({
      query: { ...filter, limit: 500, cursor },
    });
    if (error) throw new Error(error.message);

    yield* data.entries;
    cursor = data.next_cursor;
  } while (cursor != null);
}
```

Check `cursor != null`, not truthiness, so it keeps working if an id were ever 0.

## Streaming logs

`streamLogs` is a server-sent event stream: first the cached log entries, oldest first (up to `--log-buffer-lines`,
default 1000, so a new connection can receive that many at once), then each new entry as it is logged.

Despite the `EventSource` advice in the spec's description, the generated client does not use `EventSource`. It uses
`fetch`, with its own parsing, reconnecting and `Last-Event-ID` handling. The function returns a `stream`, an async
generator that yields **already parsed, typed `LogEntry` values**; there is nothing to cast.

```ts
import { streamLogs } from './client';

const controller = new AbortController();
const { stream } = await streamLogs({ signal: controller.signal });

for await (const entry of stream) {
  console.log(entry.level, entry.target, entry.message, entry.fields);
}

// elsewhere, to stop: controller.abort()
```

Aborting ends the loop normally; it doesn't throw.

A `LogEntry` has `id`, `timestamp`, `level` (`'trace' | 'debug' | 'info' | 'warn' | 'error'`), `target` (the module that
logged it), `message` (empty if there was none), `fields` and `spans`. Both `fields` and the `fields` of each span are
`Record<string, string>`: values arrive rendered as text, so `attempts = 3` is `"3"`. `spans` is outermost first;
handlers run in a span carrying the `user_id` and `document`, for example.

### What the client does on its own

| Situation                                      | Behaviour                                                                                 |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------- |
| You abort the signal                           | The loop ends without an error. If it is waiting out a retry delay, that delay finishes first. |
| The connection drops, or the server is killed  | `onSseError` is called, then it retries with exponential backoff and sends `Last-Event-ID`, so the stream **continues where it left off**. |
| The server comes back after a restart          | It reconnects by itself. The server ignores a `Last-Event-ID` from before its restart and replays its new cache, so entries start again from `id` 1. |
| **The server shuts down gracefully**           | The stream **ends cleanly and the loop falls through. It does not reconnect.**            |
| The URL is wrong (a 404, a wrong path prefix)  | `onSseError` is called with `SSE failed: 404 Not Found`, and it **retries forever** with backoff. Nothing is thrown. |

The graceful-shutdown row is the one that catches people out: `docker restart`, a deploy, or a plain `Ctrl-C` all end
the stream the way a normal close does, and a browser `EventSource` would have reconnected where this client doesn't.

Retry timing is configurable per call:

```ts
await streamLogs({
  signal,
  sseDefaultRetryDelay: 1000, // first retry after 1s, default 3000
  sseMaxRetryDelay: 10_000, // doubling up to this, default 30000
  sseMaxRetryAttempts: 5, // give up after this many failures
  onSseError: (error) => console.warn('log stream error', error),
});
```

`sseMaxRetryAttempts` counts failures over the life of the call and is never reset by a successful reconnect, so over a
long-lived stream it stops being "5 in a row". Leave it unset unless you want the stream to give up eventually, and
show the user the errors from `onSseError`, because nothing else reports a stream that can't connect.

### Following logs across restarts

Wrap the stream in a loop so a clean end becomes a reconnect. Passing the last event id along on the way back keeps a
reconnect to the same server run from replaying entries you already have:

```ts
import { streamLogs, type LogEntry } from './client';

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function* followLogs(signal: AbortSignal): AsyncGenerator<LogEntry> {
  let lastEventId: string | undefined;

  while (!signal.aborted) {
    const { stream } = await streamLogs({
      signal,
      headers: lastEventId ? { 'Last-Event-ID': lastEventId } : undefined,
      onSseEvent: (event) => {
        lastEventId = event.id ?? lastEventId;
      },
    });

    yield* stream;

    // A clean end means the server closed the stream, as it does when it
    // shuts down, and the client does not reconnect by itself in that case
    if (!signal.aborted) await sleep(2000);
  }
}
```

While the server is down, reconnection attempts inside `streamLogs` are handled by the client's own retries, so this
loop only runs again after a clean end.

### Entry ids

There are two ids and they are not the same thing:

- **`entry.id`** is a number, consecutive within one run of the server and starting at 1. A **gap means the client fell
  too far behind and entries were skipped**.
- **The SSE event id** (`event.id` in `onSseEvent`) is the string `"<run>-<entry id>"`. The run part is what lets the
  server tell a `Last-Event-ID` from this run from one from before a restart. Treat it as opaque and only send it
  back.

Because `entry.id` starts over at 1 when the server restarts, **don't use it alone as a key** in a list that survives a
reconnect: the new run's entry 1 collides with the old run's entry 1. Either clear the list when the stream
reconnects after an error, or key on a counter of your own.

Whatever you display, cap how many entries you keep. The stream has no end, and a server running with a verbose
`RUST_LOG` can log a lot.

## Checklist

- [ ] The base URL is right for where the page is served. The default is relative to the page's origin; set it with
      `client.setConfig`, including the path prefix, for another origin or outside a browser.
- [ ] The page's origin is in the server's `--admin-cors-origins`, if it is not served from the same origin.
- [ ] Errors are normalized with an interceptor, since a network failure doesn't reject and a proxy's error page
      arrives as a string.
- [ ] Nullable fields are handled, and timestamps are parsed from strings.
- [ ] A log stream is wrapped so that a clean end reconnects, and `onSseError` is surfaced to the user.
- [ ] The list of log entries is bounded and doesn't rely on `entry.id` alone being unique.
