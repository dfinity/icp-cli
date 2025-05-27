use std::fmt::Display;

use icp_dirs::IcpCliDirs;
use serde::Serialize;

use crate::OutputFormat;

pub struct Env {
    output_format: OutputFormat,
    dirs: IcpCliDirs,
}

impl Env {
    pub fn new(output_format: OutputFormat) -> Self {
        Self {
            output_format,
            dirs: IcpCliDirs::new(),
        }
    }
    pub fn _output_format(&self) -> OutputFormat {
        self.output_format
    }
    pub fn print_result<E>(&self, res: Result<impl Display + Serialize, E>) -> Result<(), E> {
        match res {
            Err(e) => Err(e),
            Ok(o) => match self.output_format {
                OutputFormat::Human => {
                    println!("{o}");
                    Ok(())
                }
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string(&o).unwrap());
                    Ok(())
                }
            },
        }
    }
    pub fn dirs(&self) -> &IcpCliDirs {
        &self.dirs
    }
}
