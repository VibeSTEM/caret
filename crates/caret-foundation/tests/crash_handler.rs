#[cfg(unix)]
#[test]
fn install_reports_log_file_open_failure() -> std::io::Result<()> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "caret-foundation-crash-open-failure-{}-{nonce}",
        std::process::id()
    ));
    let root_text = root.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "temporary path is not UTF-8",
        )
    })?;
    std::fs::create_dir(&root)?;
    let log_target = root.join(format!("caret-native-crash-{}.log", std::process::id()));
    std::fs::create_dir(&log_target)?;

    let result = caret_foundation::crash::install_crash_handler(root_text);
    std::fs::remove_dir_all(root)?;

    let error = match result {
        Ok(path) => {
            return Err(std::io::Error::other(format!(
                "directory at log path was accepted: {}",
                path.display()
            )));
        }
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::IsADirectory);
    Ok(())
}
