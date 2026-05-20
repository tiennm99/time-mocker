use std::mem::size_of;

/// 8-byte payload of the shared MMF.
///
/// `delta_ticks` is added to the real FILETIME (100-ns since 1601-01-01 UTC)
/// by every hooked time API call. May be negative to mock past times.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MockTimeInfo {
    pub delta_ticks: i64,
}

impl MockTimeInfo {
    pub const SIZE: usize = size_of::<Self>();

    #[inline]
    pub const fn zero() -> Self {
        Self { delta_ticks: 0 }
    }
}

const _: () = assert!(MockTimeInfo::SIZE == 8);
