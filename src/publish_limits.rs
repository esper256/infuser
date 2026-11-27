//! Limits shared by web build admission and publisher preflight.
pub const MIB: u64 = 1024 * 1024;
pub const FILE_BYTES: u64 = 128 * MIB;
pub const BUILD_BYTES: u64 = 512 * MIB;
pub const FILE_COUNT: usize = 8192;
pub const UPLOAD_SECONDS: u64 = 240;
pub const ADMISSION_SLOTS: usize = 2;
