use std::path::Path;

use openclicky_core::error::{Error, Result};
use openclicky_core::ports::{CommandOutput, CommandRunner};

pub struct SystemCommands;

impl CommandRunner for SystemCommands {
    fn run(&self, program: &str, args: &[String], cwd: &Path) -> Result<CommandOutput> {
        let out = std::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|e| Error::Io(format!("{program}: {e}")))?;
        Ok(CommandOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    }
}
