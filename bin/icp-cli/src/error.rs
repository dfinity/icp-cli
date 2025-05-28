use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use snafu::FromString;

#[derive(Debug)]
pub struct AnyError {
    message: Option<String>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl<E: Error + Send + Sync + 'static> From<E> for AnyError {
    fn from(value: E) -> Self {
        Self {
            message: None,
            source: Some(Box::new(value)),
        }
    }
}

impl Display for AnyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(message) = &self.message {
            message.fmt(f)
        } else {
            self.source.as_ref().unwrap().fmt(f)
        }
    }
}

impl FromString for AnyError {
    type Source = Box<dyn Error + Send + Sync>;
    fn with_source(source: Self::Source, message: String) -> Self {
        Self {
            message: Some(message),
            source: Some(source),
        }
    }
    fn without_source(message: String) -> Self {
        Self {
            message: Some(message),
            source: None,
        }
    }
}

#[derive(Debug)]
pub struct AnyErrorCompat(pub AnyError);

impl Display for AnyErrorCompat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Error for AnyErrorCompat {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.0.source.as_ref().and_then(|x| x.source())
    }
}
