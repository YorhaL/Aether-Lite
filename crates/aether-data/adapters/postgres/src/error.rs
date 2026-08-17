use crate::DataLayerError;

pub(crate) fn postgres_error(error: sqlx::Error) -> DataLayerError {
    let sqlstate = error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .map(|code| code.into_owned());
    match sqlstate {
        Some(code) => DataLayerError::Postgres(format!("{error} (SQLSTATE {code})")),
        None => DataLayerError::postgres(error),
    }
}

pub(crate) trait SqlxResultExt<T> {
    fn map_postgres_err(self) -> Result<T, DataLayerError>;
}

impl<T> SqlxResultExt<T> for Result<T, sqlx::Error> {
    fn map_postgres_err(self) -> Result<T, DataLayerError> {
        self.map_err(postgres_error)
    }
}

#[cfg(test)]
mod tests {
    use super::postgres_error;
    use sqlx::error::{DatabaseError, ErrorKind};
    use std::borrow::Cow;
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    #[derive(Debug)]
    struct TestDatabaseError;

    impl Display for TestDatabaseError {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("unsupported Unicode escape sequence")
        }
    }

    impl Error for TestDatabaseError {}

    impl DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            "unsupported Unicode escape sequence"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed("22P05"))
        }

        fn as_error(&self) -> &(dyn Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    #[test]
    fn postgres_error_preserves_sqlstate_without_database_detail() {
        let mapped = postgres_error(sqlx::Error::Database(Box::new(TestDatabaseError)));
        assert_eq!(
            mapped.to_string(),
            "postgres error: error returned from database: unsupported Unicode escape sequence (SQLSTATE 22P05)"
        );
    }

    #[test]
    fn postgres_error_without_database_code_keeps_original_message() {
        let mapped = postgres_error(sqlx::Error::PoolTimedOut);
        assert_eq!(
            mapped.to_string(),
            "postgres error: pool timed out while waiting for an open connection"
        );
    }
}
