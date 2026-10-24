//! Caret 进程级基础设施。
//!
//! 该 crate 不承载工程、任务或几何领域模型；它提供需要在应用进程最早期
//! 初始化的操作系统边界，例如崩溃信号处理。手写 unsafe 只允许集中在对应
//! 的专用模块中，公共入口保持安全签名。

// SAFETY: `crash` 是仓库登记的唯一手写崩溃信号 unsafe 边界；模块内部每个
// unsafe 块均以 `// SAFETY:` 说明指针、FD、线程和信号处理前提。
#[allow(unsafe_code)]
pub mod crash;
