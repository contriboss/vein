use anyhow::Result;
use goblin::{
    elf::header::EM_386, elf::header::EM_AARCH64, elf::header::EM_ARM, elf::header::EM_X86_64,
    mach::cputype::CPU_TYPE_ARM64, mach::cputype::CPU_TYPE_X86_64, Object,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectedArch {
    X86,
    X86_64,
    Arm64,
    Arm,
    Unknown(String),
}

impl DetectedArch {
    pub fn as_str(&self) -> &str {
        match self {
            DetectedArch::X86 => "x86",
            DetectedArch::X86_64 => "x86_64",
            DetectedArch::Arm64 => "arm64",
            DetectedArch::Arm => "arm",
            DetectedArch::Unknown(s) => s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectedOS {
    Linux,
    Darwin,
    Windows,
    Unknown,
}

impl DetectedOS {
    pub fn as_str(&self) -> &str {
        match self {
            DetectedOS::Linux => "linux",
            DetectedOS::Darwin => "darwin",
            DetectedOS::Windows => "windows",
            DetectedOS::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryInfo {
    pub arch: DetectedArch,
    pub os: DetectedOS,
}

impl BinaryInfo {
    pub fn platform_string(&self) -> String {
        format!("{}-{}", self.arch.as_str(), self.os.as_str())
    }
}

/// Parse binary file and extract architecture info
pub fn detect_binary_arch(data: &[u8]) -> Result<BinaryInfo> {
    let obj = Object::parse(data)?;

    match obj {
        Object::Elf(elf) => {
            let arch = match elf.header.e_machine {
                EM_X86_64 => DetectedArch::X86_64,
                EM_AARCH64 => DetectedArch::Arm64,
                EM_386 => DetectedArch::X86,
                EM_ARM => DetectedArch::Arm,
                other => DetectedArch::Unknown(format!("elf_{}", other)),
            };

            Ok(BinaryInfo {
                arch,
                os: DetectedOS::Linux,
            })
        }
        Object::Mach(mach) => {
            use goblin::mach::Mach;

            let arch = match mach {
                Mach::Binary(macho) => match macho.header.cputype() {
                    CPU_TYPE_ARM64 => DetectedArch::Arm64,
                    CPU_TYPE_X86_64 => DetectedArch::X86_64,
                    other => DetectedArch::Unknown(format!("macho_{}", other)),
                },
                Mach::Fat(fat) => {
                    // For fat binaries, check the first arch
                    if let Some(arch) = fat.arches()?.first() {
                        match arch.cputype() {
                            CPU_TYPE_ARM64 => DetectedArch::Arm64,
                            CPU_TYPE_X86_64 => DetectedArch::X86_64,
                            other => DetectedArch::Unknown(format!("macho_fat_{}", other)),
                        }
                    } else {
                        DetectedArch::Unknown("macho_fat_empty".to_string())
                    }
                }
            };

            Ok(BinaryInfo {
                arch,
                os: DetectedOS::Darwin,
            })
        }
        Object::PE(pe) => {
            // PE machine type constants
            const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
            const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;
            const IMAGE_FILE_MACHINE_I386: u16 = 0x014c;

            let arch = match pe.header.coff_header.machine {
                IMAGE_FILE_MACHINE_AMD64 => DetectedArch::X86_64,
                IMAGE_FILE_MACHINE_ARM64 => DetectedArch::Arm64,
                IMAGE_FILE_MACHINE_I386 => DetectedArch::X86,
                other => DetectedArch::Unknown(format!("pe_{:#x}", other)),
            };

            Ok(BinaryInfo {
                arch,
                os: DetectedOS::Windows,
            })
        }
        _ => anyhow::bail!("Unsupported binary format"),
    }
}

/// Parse gem platform string into components
/// Examples: "arm64-darwin" -> (Arm64, Darwin)
///           "x86_64-linux" -> (X86_64, Linux)
///           "x86_64-linux-musl" -> (X86_64, Linux)
pub fn parse_gem_platform(platform: &str) -> Option<(DetectedArch, DetectedOS)> {
    let parts: Vec<&str> = platform.split('-').collect();
    if parts.is_empty() {
        return None;
    }

    let arch = match parts[0] {
        "x86_64" | "x64" => DetectedArch::X86_64,
        "aarch64" | "arm64" => DetectedArch::Arm64,
        "x86" | "i386" | "i686" => DetectedArch::X86,
        "arm" => DetectedArch::Arm,
        other => DetectedArch::Unknown(other.to_string()),
    };

    let os = if parts.len() > 1 {
        match parts[1] {
            "linux" => DetectedOS::Linux,
            "darwin" => DetectedOS::Darwin,
            "mingw32" | "mingw" | "mswin" | "windows" => DetectedOS::Windows,
            _ => DetectedOS::Unknown,
        }
    } else {
        DetectedOS::Unknown
    };

    Some((arch, os))
}

/// Check if detected binary info matches the claimed gem platform
pub fn matches_platform(claimed: Option<&str>, detected: &BinaryInfo) -> bool {
    let Some(platform) = claimed else {
        // No claimed platform (pure ruby gem) - any binary is invalid
        return false;
    };

    // Special case: "ruby" or "java" platforms can have any architecture
    if platform == "ruby" || platform == "java" {
        return true;
    }

    let Some((claimed_arch, claimed_os)) = parse_gem_platform(platform) else {
        return false;
    };

    // OS must match exactly
    if claimed_os != detected.os {
        return false;
    }

    // Architecture must match (including aliases like aarch64 == arm64)
    matches!(
        (&claimed_arch, &detected.arch),
        (DetectedArch::Arm64, DetectedArch::Arm64)
            | (DetectedArch::X86_64, DetectedArch::X86_64)
            | (DetectedArch::X86, DetectedArch::X86)
            | (DetectedArch::Arm, DetectedArch::Arm)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gem_platform() {
        assert_eq!(
            parse_gem_platform("arm64-darwin"),
            Some((DetectedArch::Arm64, DetectedOS::Darwin))
        );
        assert_eq!(
            parse_gem_platform("x86_64-linux"),
            Some((DetectedArch::X86_64, DetectedOS::Linux))
        );
        assert_eq!(
            parse_gem_platform("x86_64-linux-musl"),
            Some((DetectedArch::X86_64, DetectedOS::Linux))
        );
        assert_eq!(
            parse_gem_platform("aarch64-linux"),
            Some((DetectedArch::Arm64, DetectedOS::Linux))
        );
    }

    #[test]
    fn test_matches_platform() {
        let arm64_darwin = BinaryInfo {
            arch: DetectedArch::Arm64,
            os: DetectedOS::Darwin,
        };

        assert!(matches_platform(Some("arm64-darwin"), &arm64_darwin));
        assert!(matches_platform(Some("aarch64-darwin"), &arm64_darwin));
        assert!(!matches_platform(Some("x86_64-darwin"), &arm64_darwin));
        assert!(!matches_platform(Some("arm64-linux"), &arm64_darwin));
        assert!(!matches_platform(None, &arm64_darwin));
        assert!(matches_platform(Some("ruby"), &arm64_darwin));
    }
}
