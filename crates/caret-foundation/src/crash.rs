//! 进程级崩溃记录（047）：先向 stderr 与预打开的日志文件写出信号或
//! Windows SEH 异常码及 PID，再尝试 POSIX 回溯。POSIX 恢复默认处置并
//! 重发信号；Windows 继续默认异常处置，保留系统诊断与原始退出语义。
//!
//! 手写 unsafe 限于本模块的系统 FFI；`caret-ffi` 仅转发安全安装入口。
//!
//! 安全取舍：write/fd 操作为 async-signal-safe；backtrace* 会调用分配器，
//! 非严格安全——崩溃点位于分配器内时回溯可能缺失或阻塞。日志句柄与路径
//! 字节在安装时准备并常驻进程生命周期；除 best-effort 回溯外，POSIX
//! 处理器不解析路径、不分配内存。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#[cfg(unix)]
use std::os::fd::IntoRawFd;
#[cfg(windows)]
use std::os::windows::io::IntoRawHandle;
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(unix)]
use std::sync::atomic::AtomicI32;
use std::sync::atomic::{AtomicPtr, Ordering};

#[cfg(unix)]
/// 未安装时的哨兵 fd；安装后保存常驻 fd（不关闭，进程退出由内核回收）。
static LOG_FD: AtomicI32 = AtomicI32::new(-1);
#[cfg(windows)]
/// 未安装时的空句柄；安装后保存常驻句柄（不关闭，进程退出由系统回收）。
static LOG_HANDLE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
/// 安装期预渲染的“ log=<path>\n”字节（以 \0 结尾）；崩溃时随信号一并输出，
/// 免去处理器内的路径/分配操作。指针指向进程生命周期常驻的泄露分配。
static LOG_LINE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// 安装后的真实日志路径；仅由非信号上下文查询，处理器使用 LOG_LINE 副本。
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 触发日志落地的崩溃信号集合（顺序即文档顺序）。
#[cfg(unix)]
const CRASH_SIGNALS: [libc::c_int; 6] = [
    libc::SIGSEGV,
    libc::SIGBUS,
    libc::SIGFPE,
    libc::SIGILL,
    libc::SIGABRT,
    libc::SIGTRAP,
];

// libc execinfo 接口；调用约束见 write_backtrace。
#[cfg(unix)]
unsafe extern "C" {
    fn backtrace(buffer: *mut *mut libc::c_void, size: libc::c_int) -> libc::c_int;
    fn backtrace_symbols_fd(buffer: *const *mut libc::c_void, size: libc::c_int, fd: libc::c_int);
}

/// 仅 async-signal-safe 操作：write 与手动整数格式化（不分配、不用
/// snprintf）。
#[cfg(unix)]
fn write_all(fd: i32, data: &[u8]) {
    let mut pointer = data.as_ptr();
    let mut remaining = data.len();
    // SAFETY: pointer/remaining 始终指向调用方提供的缓冲区内部，循环由
    // remaining 收敛；libc::write 为 async-signal-safe。
    unsafe {
        while remaining > 0 {
            let written = libc::write(fd, pointer.cast(), remaining);
            if written <= 0 {
                return;
            }
            let written = written as usize;
            pointer = pointer.add(written);
            remaining -= written;
        }
    }
}

#[cfg(unix)]
fn write_text(fd: i32, text: &str) {
    write_all(fd, text.as_bytes());
}

/// 在栈上格式化整数，供日志文件与 stderr 共用。
#[cfg(unix)]
fn write_number(fd: i32, mut value: u64) {
    let mut buffer = [0u8; 24];
    let mut at = buffer.len();
    loop {
        at -= 1;
        buffer[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    write_all(fd, &buffer[at..]);
}

#[cfg(unix)]
fn signal_name(signal: libc::c_int) -> &'static str {
    match signal {
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGBUS => "SIGBUS",
        libc::SIGFPE => "SIGFPE",
        libc::SIGILL => "SIGILL",
        libc::SIGABRT => "SIGABRT",
        libc::SIGTRAP => "SIGTRAP",
        _ => "UNKNOWN",
    }
}

/// 写出崩溃标识和回溯。不恢复默认处置、不重发信号，便于测试覆盖
/// 记录路径；真正的信号处理器在记录后仍按原语义终止进程。
#[cfg(unix)]
fn record_crash(signal: libc::c_int) {
    let log_fd = LOG_FD.load(Ordering::SeqCst);
    let log_line = LOG_LINE.load(Ordering::SeqCst);
    if log_fd >= 0 {
        write_text(log_fd, "caret-native crash: ");
        write_text(log_fd, signal_name(signal));
        write_text(log_fd, " pid=");
        write_number(log_fd, u64::from(std::process::id()));
        write_text(log_fd, "\n");
    }
    write_text(libc::STDERR_FILENO, "caret-native crash: ");
    write_text(libc::STDERR_FILENO, signal_name(signal));
    write_text(libc::STDERR_FILENO, " pid=");
    write_number(libc::STDERR_FILENO, u64::from(std::process::id()));
    if !log_line.is_null() {
        // SAFETY: 指向安装期泄露的 NUL 结尾缓冲区，进程生命周期内有效。
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(log_line.cast());
            write_all(libc::STDERR_FILENO, cstr.to_bytes());
        }
    }
    // 回溯可能触发分配器；两个目的地的基本记录必须先写出。
    if log_fd >= 0 {
        write_backtrace(log_fd);
    }
    write_backtrace(libc::STDERR_FILENO);
}

#[cfg(unix)]
extern "C" fn crash_handler(signal: libc::c_int) {
    record_crash(signal);
    // SAFETY: 恢复默认处置后重发同一信号，保持 .ips 崩溃报告与内核退出
    // 语义；signal/raise 均为 async-signal-safe。
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

#[cfg(unix)]
fn write_backtrace(fd: i32) {
    // SAFETY: backtrace/backtrace_symbols_fd 会分配内存，非严格
    // async-signal-safe——崩溃点在分配器内时可能无回溯或阻塞；
    // buffer 为本栈帧上的定长数组，fd 为预打开日志或标准错误。
    unsafe {
        let mut buffer = [std::ptr::null_mut::<libc::c_void>(); 64];
        let count = backtrace(buffer.as_mut_ptr(), 64);
        if count > 0 {
            write_text(fd, "backtrace(unsymbolized; 符号对照见 .ips):\n");
            backtrace_symbols_fd(buffer.as_ptr(), count, fd);
        }
    }
}

/// 安装处理器并创建日志文件；log_dir 为空时使用系统临时目录。返回日志
/// 路径；重复安装以最后一次为准。错误仅来自目录创建或文件打开失败。
///
/// # Errors
/// 日志目录创建失败或日志文件打开失败（权限/路径不可用）。
#[cfg(unix)]
pub fn install_crash_handler(log_dir: &str) -> std::io::Result<PathBuf> {
    let dir = if log_dir.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(log_dir)
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("caret-native-crash-{}.log", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    // SAFETY: fd 有意常驻进程生命周期——处理器在任意线程/时机写入，不
    // 关闭、不交给其他所有者；进程退出由内核回收。
    let fd = file.into_raw_fd();
    let log_line = match std::ffi::CString::new(format!(" log={}\n", path.display())) {
        Ok(line) => line,
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "日志路径包含 NUL",
            ));
        }
    };
    // SAFETY: CString 以进程生命周期泄露，处理器只读取其 NUL 结尾字节；重复
    // 安装时旧缓冲区也不能立即释放，因为已有信号处理器可能正在读取它。
    let log_line = log_line.into_raw().cast::<u8>();
    LOG_LINE.store(log_line, Ordering::SeqCst);
    if let Ok(mut current) = LOG_PATH.lock() {
        *current = Some(path.clone());
    }
    LOG_FD.store(fd, Ordering::SeqCst);
    for signal in CRASH_SIGNALS {
        // SAFETY: 注册进程级崩溃处理器；signal()（BSD 语义）保持处理器
        // 安装，处理器内部恢复默认处置并重发，.ips 崩溃报告链路保留。
        unsafe {
            libc::signal(
                signal,
                crash_handler as extern "C" fn(libc::c_int) as usize as libc::sighandler_t,
            );
        }
    }
    Ok(path)
}

#[cfg(windows)]
const STD_ERROR_HANDLE: u32 = 0xffff_fff4;

// Win32 ABI 布局；只读取异常码，保留完整记录以准确表达系统提供的类型。
#[cfg(windows)]
#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *const ExceptionRecord,
    address: *const std::ffi::c_void,
    parameter_count: u32,
    information: [usize; 15],
}

#[cfg(windows)]
#[repr(C)]
struct ExceptionPointers {
    record: *const ExceptionRecord,
    context: *const std::ffi::c_void,
}

#[cfg(windows)]
unsafe extern "system" {
    fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
    fn SetUnhandledExceptionFilter(
        filter: Option<unsafe extern "system" fn(*const ExceptionPointers) -> i32>,
    ) -> Option<unsafe extern "system" fn(*const ExceptionPointers) -> i32>;
    fn WriteFile(
        handle: *mut std::ffi::c_void,
        buffer: *const std::ffi::c_void,
        bytes_to_write: u32,
        bytes_written: *mut u32,
        overlapped: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(windows)]
fn write_all_windows(handle: *mut std::ffi::c_void, mut data: &[u8]) {
    if handle.is_null() {
        return;
    }
    while !data.is_empty() {
        let chunk_len = data.len().min(u32::MAX as usize) as u32;
        let mut written = 0u32;
        // SAFETY: handle 来自安装期常驻的文件句柄或 Windows 标准错误句柄；
        // buffer/长度指向仍存活的当前切片，overlapped 为空表示同步写出。
        let success = unsafe {
            WriteFile(
                handle,
                data.as_ptr().cast(),
                chunk_len,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if success == 0 || written == 0 {
            return;
        }
        data = &data[written as usize..];
    }
}

#[cfg(windows)]
fn write_number_windows(handle: *mut std::ffi::c_void, mut value: u64) {
    let mut buffer = [0u8; 24];
    let mut at = buffer.len();
    loop {
        at -= 1;
        buffer[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    write_all_windows(handle, &buffer[at..]);
}

#[cfg(windows)]
fn write_exception_code_windows(handle: *mut std::ffi::c_void, mut code: u32) {
    let mut buffer = *b"0x00000000";
    for at in (2..buffer.len()).rev() {
        buffer[at] = b"0123456789ABCDEF"[(code & 0xf) as usize];
        code >>= 4;
    }
    write_all_windows(handle, &buffer);
}

#[cfg(windows)]
fn write_windows_header(handle: *mut std::ffi::c_void, code: Option<u32>) {
    write_all_windows(handle, b"caret-native crash: Windows SEH code=");
    if let Some(code) = code {
        write_exception_code_windows(handle, code);
    } else {
        write_all_windows(handle, b"UNKNOWN");
    }
    write_all_windows(handle, b" pid=");
    write_number_windows(handle, u64::from(std::process::id()));
}

#[cfg(windows)]
unsafe extern "system" fn windows_exception_handler(exception: *const ExceptionPointers) -> i32 {
    // SAFETY: Windows 回调期间提供有效的异常记录；空指针仅作为防御性路径。
    let code = unsafe {
        exception
            .as_ref()
            .and_then(|p| p.record.as_ref())
            .map(|r| r.code)
    };
    let log_handle = LOG_HANDLE.load(Ordering::SeqCst);
    let log_line = LOG_LINE.load(Ordering::SeqCst);
    if !log_handle.is_null() {
        write_windows_header(log_handle, code);
        write_all_windows(log_handle, b"\n");
    }
    // SAFETY: Windows 在未处理异常回调期间提供标准错误句柄；日志路径指针由
    // 安装期泄露并在进程生命周期内保持有效。
    let stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    write_windows_header(stderr, code);
    if !log_line.is_null() {
        // SAFETY: 指向安装期泄露的 NUL 结尾缓冲区，进程生命周期内有效。
        let cstr = unsafe { std::ffi::CStr::from_ptr(log_line.cast()) };
        write_all_windows(stderr, cstr.to_bytes());
    }
    // EXCEPTION_CONTINUE_SEARCH：保留 Windows 默认 WER/minidump 处置。
    0
}

/// Windows 不使用 POSIX 信号；注册最小 SEH 顶层过滤器，写出信号等价的
/// 进程级异常记录后继续交给 WER。日志文件仍在安装期预创建并常驻打开。
///
/// # Errors
/// 日志目录创建失败或日志文件打开失败（权限/路径不可用）。
#[cfg(windows)]
pub fn install_crash_handler(log_dir: &str) -> std::io::Result<PathBuf> {
    let dir = if log_dir.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(log_dir)
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("caret-native-crash-{}.log", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    // SAFETY: 句柄有意常驻进程生命周期；异常过滤器在任意线程触发时写入，
    // 不关闭、不交给其他所有者，进程退出由 Windows 回收。
    let handle = file.into_raw_handle().cast();
    let log_line = std::ffi::CString::new(format!(" log={}\n", path.display()))
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "日志路径包含 NUL"))?;
    // SAFETY: CString 以进程生命周期泄露，过滤器只读取其 NUL 结尾字节；重复
    // 安装时旧缓冲区也不能立即释放，因为旧回调可能正在读取它。
    let log_line = log_line.into_raw().cast::<u8>();
    LOG_LINE.store(log_line, Ordering::SeqCst);
    if let Ok(mut current) = LOG_PATH.lock() {
        *current = Some(path.clone());
    }
    LOG_HANDLE.store(handle, Ordering::SeqCst);
    // SAFETY: 注册进程级 Windows 异常过滤器；返回 0 保留默认 WER/minidump。
    unsafe {
        SetUnhandledExceptionFilter(Some(windows_exception_handler));
    }
    Ok(path)
}

/// 当前崩溃日志路径（未安装返回 None）。
#[must_use]
pub fn crash_log_path() -> Option<PathBuf> {
    LOG_PATH.lock().ok().and_then(|current| current.clone())
}

// Miri 不支持子进程/信号与 Win32 FFI 等进程边界调用；这些测试只在真实平台
// 执行，Miri 下仍编译 crash 模块本体。
#[cfg(all(test, not(miri)))]
mod tests;
