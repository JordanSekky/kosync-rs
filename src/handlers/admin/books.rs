use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    db::{Db, datetime_from_ns},
    error::Result,
    extract::AppQuery,
    models::Book,
};

#[derive(Deserialize, JsonSchema, Debug)]
pub struct ListBooksQuery {
    /// Only documents this user has synced
    user_id: Option<i64>,
}

#[derive(Serialize, JsonSchema, Debug)]
pub struct BookResponse {
    /// The document's id, which the client derives from the file
    document: String,
    /// Null unless a client sent metadata for the document
    title: Option<String>,
    authors: Option<String>,
    filename: Option<String>,
    /// Number of users that have synced the document
    reader_count: i64,
    /// Number of progress history entries for the document
    entry_count: i64,
    last_activity_at: DateTime<Utc>,
}

impl From<Book> for BookResponse {
    fn from(book: Book) -> Self {
        Self {
            document: book.document,
            title: book.title,
            authors: book.authors,
            filename: book.filename,
            reader_count: book.reader_count,
            entry_count: book.entry_count,
            last_activity_at: datetime_from_ns(book.last_activity_ns),
        }
    }
}

#[derive(Serialize, JsonSchema, Debug)]
pub struct ListBooksResponse {
    /// Most recently synced first
    books: Vec<BookResponse>,
}

#[tracing::instrument(skip_all)]
pub async fn list(
    State(db): State<Db>,
    AppQuery(query): AppQuery<ListBooksQuery>,
) -> Result<Json<ListBooksResponse>> {
    let books = Book::list(&db, query.user_id).await?;

    Ok(Json(ListBooksResponse {
        books: books.into_iter().map(Into::into).collect(),
    }))
}
