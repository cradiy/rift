use std::fs;

use anyhow::{Context, Result};
use flexi_logger::{
    Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming, colored_detailed_format,
    detailed_format,
};
use rift_config::LoadedConfig;

pub(crate) fn initialize(config: &LoadedConfig) -> Result<()> {
    let log_directory = config.log_directory()?;
    fs::create_dir_all(&log_directory)
        .with_context(|| format!("failed to create log directory {}", log_directory.display()))?;
    let duplicate = if config.config.logging.console {
        Duplicate::All
    } else {
        Duplicate::None
    };

    Logger::try_with_env_or_str(config.config.logging.filter_spec())?
        .log_to_file(
            FileSpec::default()
                .directory(log_directory)
                .basename("rift"),
        )
        .duplicate_to_stdout(duplicate)
        .format_for_stdout(colored_detailed_format)
        .format_for_files(detailed_format)
        .set_palette("196;208;40;39;244".to_owned())
        .rotate(
            Criterion::Size(config.config.logging.max_file_size_bytes),
            Naming::Numbers,
            Cleanup::KeepLogFiles(config.config.logging.retained_files),
        )
        .start()?;
    Ok(())
}
