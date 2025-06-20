use clap::{Args, ValueEnum};

#[derive(Args, Debug)]
pub struct WithFormat {
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Format {
    Json,
    Text,
}
