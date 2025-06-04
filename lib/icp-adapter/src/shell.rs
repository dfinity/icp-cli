#[derive(Copy, Clone)]
pub enum Shell {
    Unix,
    Windows,
}

// Compile-time constant selection of shell based on platform
#[cfg(target_family = "unix")]
pub const SHELL: Shell = Shell::Unix;

#[cfg(target_family = "windows")]
pub const SHELL: Shell = Shell::Windows;

impl Shell {
    pub fn binary(self) -> &'static str {
        match self {
            Shell::Unix => "/bin/sh",
            Shell::Windows => "cmd",
        }
    }

    pub fn exec_flag(self) -> &'static str {
        match self {
            Shell::Unix => "-c",
            Shell::Windows => "/C",
        }
    }
}
