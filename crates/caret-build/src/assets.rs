//! 固定工具版本、官方资产 URL 与 SHA256；不执行安装或平台进程。

/// 单个工具的官方资产：URL 与首次下载实测的 SHA256（升级时同步回填
/// standards/dependency-acquisition.md）。
pub(super) struct ToolAsset {
    pub(super) url: &'static str,
    pub(super) sha256: &'static str,
}

const CMAKE_VERSION: &str = "4.4.3";
const NINJA_VERSION: &str = "1.13.2";
pub const LLVM_VERSION: &str = "22.1.7";

pub const UV_VERSION: &str = "0.12.18";
pub const HYPERFINE_VERSION: &str = "2.0.0";

fn hyperfine_asset() -> Option<ToolAsset> {
    let (url, sha256) = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        (
            "https://github.com/sharkdp/hyperfine/releases/download/v2.0.0/hyperfine-v2.0.0-aarch64-apple-darwin.tar.gz",
            "590b839cfea386b384ce0b42d16daf2910277cf30950656194bb3a5f20dea81e",
        )
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        (
            "https://github.com/sharkdp/hyperfine/releases/download/v2.0.0/hyperfine-v2.0.0-x86_64-unknown-linux-gnu.tar.gz",
            "ae2beda2ac99c098427e4f755244552daf20e514422889cca6d44421e2d93c87",
        )
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        (
            "https://github.com/sharkdp/hyperfine/releases/download/v2.0.0/hyperfine-v2.0.0-x86_64-pc-windows-msvc.zip",
            "3dec51acee1060908e67948838d6bc0a7aacde0774acebfa4afb220dcf0bc078",
        )
    } else {
        return None;
    };
    Some(ToolAsset { url, sha256 })
}

fn cmake_asset() -> Option<ToolAsset> {
    // macOS 资产为 universal（arm64/x86_64）；Linux/Windows 官方仅 x86_64，
    // 其他架构返回 None 走诊断（见 resolve_cmake）。
    if cfg!(target_os = "macos") {
        Some(ToolAsset {
            url: "https://github.com/Kitware/CMake/releases/download/v4.4.3/cmake-4.4.3-macos-universal.tar.gz",
            sha256: "0c5d65251c14cc884bfa16bdbed3c263ce5bffe2e21c0d0d00962cb0610464fa",
        })
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some(ToolAsset {
            url: "https://github.com/Kitware/CMake/releases/download/v4.4.3/cmake-4.4.3-linux-x86_64.tar.gz",
            sha256: "d6c83076c575bc00b823522ac974bda66d0af05d6ddc30e739c12385cf32c6cc",
        })
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some(ToolAsset {
            url: "https://github.com/Kitware/CMake/releases/download/v4.4.3/cmake-4.4.3-windows-x86_64.zip",
            sha256: "4d52ebab7193a698651639ed80d8d04fd903358843572cf44c7fd234cb7c26ab",
        })
    } else {
        None
    }
}

fn ninja_asset() -> Option<ToolAsset> {
    if cfg!(target_os = "macos") {
        Some(ToolAsset {
            url: "https://github.com/ninja-build/ninja/releases/download/v1.13.2/ninja-mac.zip",
            sha256: "c99048673aa765960a99cf10c6ddb9f1fad506099ff0a0e137ad8960a88f321b",
        })
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some(ToolAsset {
            url: "https://github.com/ninja-build/ninja/releases/download/v1.13.2/ninja-linux.zip",
            sha256: "5749cbc4e668273514150a80e387a957f933c6ed3f5f11e03fb30955e2bbead6",
        })
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some(ToolAsset {
            url: "https://github.com/ninja-build/ninja/releases/download/v1.13.2/ninja-win.zip",
            sha256: "07fc8261b42b20e71d1720b39068c2e14ffcee6396b76fb7a795fb460b78dc65",
        })
    } else {
        None
    }
}

/// LLVM 官方发布资产。Windows 使用官方 NSIS 安装包：它包含 clang-cl、lld-link
/// 与 clang-format，静默安装到 Cargo 的托管目录，不写入用户系统目录；安装经
/// RunAsInvoker 兼容层运行，非提权 shell 无需管理员权限。
fn llvm_asset() -> Option<ToolAsset> {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Some(ToolAsset {
            url: "https://github.com/llvm/llvm-project/releases/download/llvmorg-22.1.7/LLVM-22.1.7-macOS-ARM64.tar.xz",
            sha256: "4177245188b0a30a6539c96b361dea56f253485756bfd8927a6a59e7301e7806",
        })
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some(ToolAsset {
            url: "https://github.com/llvm/llvm-project/releases/download/llvmorg-22.1.7/LLVM-22.1.7-Linux-X64.tar.xz",
            sha256: "edb0522b41e261819c06ea437d249f9b8acfa413d3805bc9920eec6fb76ff830",
        })
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        Some(ToolAsset {
            url: "https://github.com/llvm/llvm-project/releases/download/llvmorg-22.1.7/LLVM-22.1.7-win64.exe",
            sha256: "e091fcf965ce589c83c0f7c5356b2fcf3e658a8ec990bfcf79cce4389a0d1eb3",
        })
    } else {
        None
    }
}

#[derive(Clone, Copy)]
pub(super) enum Tool {
    Cmake,
    Ninja,
    Llvm,
    Uv,
    Hyperfine,
}

impl Tool {
    pub(super) fn name(self) -> &'static str {
        match self {
            Tool::Cmake => "cmake",
            Tool::Ninja => "ninja",
            Tool::Llvm => "llvm",
            Tool::Uv => "uv",
            Tool::Hyperfine => "hyperfine",
        }
    }

    pub(super) fn version(self) -> &'static str {
        match self {
            Tool::Cmake => CMAKE_VERSION,
            Tool::Ninja => NINJA_VERSION,
            Tool::Llvm => LLVM_VERSION,
            Tool::Uv => UV_VERSION,
            Tool::Hyperfine => HYPERFINE_VERSION,
        }
    }

    pub(super) fn asset(self) -> Option<ToolAsset> {
        match self {
            Tool::Cmake => cmake_asset(),
            Tool::Ninja => ninja_asset(),
            Tool::Llvm => llvm_asset(),
            Tool::Uv => uv_asset(),
            Tool::Hyperfine => hyperfine_asset(),
        }
    }
}

pub(super) fn uv_asset() -> Option<ToolAsset> {
    let (filename, sha256) = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        (
            "uv-aarch64-apple-darwin.tar.gz",
            "cf40e0c6a202190ccd9e0406dcfdd5b2d6668a9a5c779b17948963df32aafe5b",
        )
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        (
            "uv-x86_64-unknown-linux-gnu.tar.gz",
            "89eadd7c76fc063887959510d5ba0ab1264dfd5f1143b925ddb73021a40acf16",
        )
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        (
            "uv-x86_64-pc-windows-msvc.zip",
            "cae6a3bc25239f83dffb467a4b180508d9da23986c04639ebfa44e43e6a84bff",
        )
    } else {
        return None;
    };
    let url = match filename {
        "uv-aarch64-apple-darwin.tar.gz" => {
            "https://releases.astral.sh/github/uv/releases/download/0.12.18/uv-aarch64-apple-darwin.tar.gz"
        }
        "uv-x86_64-unknown-linux-gnu.tar.gz" => {
            "https://releases.astral.sh/github/uv/releases/download/0.12.18/uv-x86_64-unknown-linux-gnu.tar.gz"
        }
        _ => {
            "https://releases.astral.sh/github/uv/releases/download/0.12.18/uv-x86_64-pc-windows-msvc.zip"
        }
    };
    Some(ToolAsset { url, sha256 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_platform_has_well_formed_assets() {
        for asset in [
            cmake_asset(),
            ninja_asset(),
            llvm_asset(),
            uv_asset(),
            hyperfine_asset(),
        ] {
            let asset = match asset {
                Some(asset) => asset,
                None => panic!("宿主平台应有固定资产"),
            };
            assert!(asset.url.starts_with("https://"));
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }
}
