use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use chrono::Local;
use log::{Level, LevelFilter, Metadata, Record};

/// 进程级文件日志后端。初始化成功后，所有 `log::*` 宏写入
/// `<app_data>/logs/at-switch.log`，便于排查切换失败 / 本地启动失败。
pub struct FileLogger {
    file: Mutex<File>,
}

impl FileLogger {
    pub fn new(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!(
            "[{}] {} {}: {}\n",
            timestamp,
            record.level(),
            record.target(),
            record.args()
        );
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// 初始化文件日志后端。日志门禁只允许设置一次，因此仅在应用 setup 调用。
pub fn init_logging(app_data_dir: &Path) -> std::io::Result<()> {
    let log_path = app_data_dir.join("logs").join("at-switch.log");
    let logger = FileLogger::new(&log_path)?;
    log::set_boxed_logger(Box::new(logger))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    log::set_max_level(LevelFilter::Info);
    Ok(())
}
