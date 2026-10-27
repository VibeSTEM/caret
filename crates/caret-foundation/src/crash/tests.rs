//! 验证崩溃处理器的进程级日志和退出行为。

use super::*;

/// 记录与安装都改进程级 fd；并行测试会互相覆盖。
#[cfg(unix)]
fn lock_crash_state() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 使用全新进程触发崩溃，避免污染父测试进程的处理器及退出状态。
#[cfg(any(unix, windows))]
#[test]
fn handler_logs_crash_and_preserves_exit() -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    let dir = std::env::temp_dir().join(format!(
        "caret-foundation-crash-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)?;
    let child = Command::new(std::env::current_exe()?)
        .args([
            "--ignored",
            "--exact",
            "crash::tests::crash_subprocess_entry",
            "--nocapture",
        ])
        .env("CARET_CRASH_TEST_DIR", &dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id();
    let output = child.wait_with_output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(output.status.signal(), Some(libc::SIGSEGV), "{stderr}");
    }
    #[cfg(windows)]
    assert_eq!(
        output.status.code().map(|code| code as u32),
        Some(0xc000_0005),
        "{stderr}"
    );
    let path = dir.join(format!("caret-native-crash-{pid}.log"));
    let content = std::fs::read_to_string(&path)?;
    #[cfg(unix)]
    let header = format!("caret-native crash: SIGSEGV pid={pid}");
    #[cfg(windows)]
    let header = format!("caret-native crash: Windows SEH code=0xC0000005 pid={pid}");
    assert_eq!(content.lines().next(), Some(header.as_str()), "{content}");
    assert!(
        stderr.starts_with(&format!("{header} log={}\n", path.display())),
        "{stderr}"
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[cfg(any(unix, windows))]
#[test]
#[ignore = "Controlled crash child; handler_logs_crash_and_preserves_exit launches this explicitly"]
fn crash_subprocess_entry() -> std::io::Result<()> {
    let dir = std::env::var("CARET_CRASH_TEST_DIR")
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let path = install_crash_handler(&dir)?;
    assert_eq!(crash_log_path().as_deref(), Some(path.as_path()));
    #[cfg(unix)]
    // SAFETY: 此测试仅在独立子进程、日志已安装后触发；信号处理器应恢复
    // 默认处置并以同一信号终止。_exit(77) 标识信号意外返回的失败路径。
    unsafe {
        libc::raise(libc::SIGSEGV);
        libc::_exit(77);
    }
    #[cfg(windows)]
    {
        // 仅抑制测试子进程的 WER 界面，保留系统异常终止路径，避免 CI 弹窗等待。
        // SAFETY: FFI 参数为文档定义的 NO_UI 标志与无附加参数的非连续异常；
        // 此入口已通过显式环境变量确认运行在独立测试子进程。
        unsafe {
            const WER_FAULT_REPORTING_NO_UI: u32 = 32;
            let result = WerSetFlags(WER_FAULT_REPORTING_NO_UI);
            assert!(result >= 0, "WerSetFlags failed: {result:#x}");
            RaiseException(0xc000_0005, 1, 0, std::ptr::null());
        }
        std::process::exit(77);
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn WerSetFlags(flags: u32) -> i32;
    fn RaiseException(code: u32, flags: u32, count: u32, arguments: *const usize);
}

#[cfg(unix)]
#[test]
fn formatting_helpers_write_expected_bytes() -> std::io::Result<()> {
    let path = std::env::temp_dir().join(format!(
        "caret-foundation-crash-format-test-{}",
        std::process::id()
    ));
    let file = std::fs::File::create(&path)?;
    // SAFETY: fd 有效且在读取前保持打开；测试结束时显式关闭，避免泄露到
    // 其他测试进程边界。
    let fd = file.into_raw_fd();
    write_text(fd, "prefix:");
    write_number(fd, 0);
    write_text(fd, ",");
    write_number(fd, 42);
    write_text(fd, ",");
    write_number(fd, u64::MAX);
    // SAFETY: fd 来自本测试刚转移所有权的 File，且仅关闭一次。
    unsafe {
        libc::close(fd);
    }
    let content = std::fs::read_to_string(&path)?;
    assert_eq!(content, format!("prefix:0,42,{}", u64::MAX));
    assert_eq!(signal_name(libc::SIGSEGV), "SIGSEGV");
    assert_eq!(signal_name(libc::SIGBUS), "SIGBUS");
    assert_eq!(signal_name(libc::SIGFPE), "SIGFPE");
    assert_eq!(signal_name(libc::SIGILL), "SIGILL");
    assert_eq!(signal_name(libc::SIGABRT), "SIGABRT");
    assert_eq!(signal_name(libc::SIGTRAP), "SIGTRAP");
    assert_eq!(signal_name(-1), "UNKNOWN");
    write_all(-1, b"ignored");
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn record_crash_writes_log_without_terminating() -> std::io::Result<()> {
    let _guard = lock_crash_state();
    let path = std::env::temp_dir().join(format!(
        "caret-foundation-crash-record-{}",
        std::process::id()
    ));
    let file = std::fs::File::create(&path)?;
    // SAFETY: fd 由本测试独占，记录完成后恢复哨兵并关闭一次。
    let fd = file.into_raw_fd();
    let previous_fd = LOG_FD.swap(fd, Ordering::SeqCst);
    let previous_line = LOG_LINE.swap(std::ptr::null_mut(), Ordering::SeqCst);
    let log_line = std::ffi::CString::new(" log=/tmp/caret-record\n")
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?
        .into_raw();
    LOG_LINE.store(log_line.cast(), Ordering::SeqCst);
    record_crash(libc::SIGBUS);
    LOG_FD.store(previous_fd, Ordering::SeqCst);
    LOG_LINE.store(previous_line, Ordering::SeqCst);
    // SAFETY: 指针来自本测试刚泄露的 CString，且处理器不再读取它。
    unsafe {
        drop(std::ffi::CString::from_raw(log_line));
        libc::close(fd);
    }
    let content = std::fs::read_to_string(&path)?;
    let header = format!("caret-native crash: SIGBUS pid={}\n", std::process::id());
    assert!(content.starts_with(&header), "{content}");
    assert!(
        content.contains("backtrace(unsymbolized; 符号对照见 .ips):\n"),
        "{content}"
    );
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn install_rejects_unusable_directory_and_accepts_empty_dir() -> std::io::Result<()> {
    let _guard = lock_crash_state();
    let root = std::env::temp_dir().join(format!(
        "caret-foundation-crash-install-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root)?;
    let blocking = root.join("not-a-directory");
    std::fs::File::create(&blocking)?;
    let blocking_text = blocking.to_str().ok_or(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "临时路径不是 UTF-8",
    ))?;
    let failed = install_crash_handler(blocking_text);
    assert!(failed.is_err(), "{failed:?}");

    let installed = install_crash_handler("")?;
    assert!(installed.is_file(), "{}", installed.display());
    assert_eq!(crash_log_path().as_deref(), Some(installed.as_path()));

    let blocked_log_dir = root.join("blocked-log");
    let blocked_log =
        blocked_log_dir.join(format!("caret-native-crash-{}.log", std::process::id()));
    std::fs::create_dir_all(blocked_log)?;
    let blocked_log_dir_text = blocked_log_dir.to_str().ok_or(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "临时路径不是 UTF-8",
    ))?;
    assert!(install_crash_handler(blocked_log_dir_text).is_err());
    assert_eq!(crash_log_path().as_deref(), Some(installed.as_path()));
    for signal in CRASH_SIGNALS {
        // SAFETY: 测试结束恢复默认处置，避免后续用例继承本进程处理器。
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
        }
    }
    std::fs::remove_file(&installed)?;
    std::fs::remove_dir_all(root)?;
    Ok(())
}
