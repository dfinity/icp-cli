use clap::ValueEnum;


#[derive(Copy, Clone, Debug, ValueEnum, Default)]
pub enum Format {
    Json,

    #[default]
    Text,
}

