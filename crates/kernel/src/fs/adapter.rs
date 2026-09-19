//! 把驱动层的块设备适配成文件系统需要的接口: 驱动只关心扇区级细节,
//! 文件系统只想要"按块号读/写一个块"。

use oslab_drivers::block::{BlockDevice as DrvBlockDevice, BlockError};

use super::bio::BlockDevice as FsBlockDevice;

// 驱动返回 `Result<(), BlockError>` (区分越界/超时/设备错误), 文件系统
// 只要 `bool`。分类信息对文件系统没用 —— 它对"读失败"的反应是统一的;
// 但诊断需要细节, 所以把最后一次错误记下来供上层报告。

/// 把驱动层的块设备适配成文件系统层的块设备。
///
/// 持有 `&mut dyn DrvBlockDevice` 而非拥有它 (设备是全局唯一一份, 由内核持有)。
pub struct BlockAdapter<'a> {
    dev: &'a mut dyn DrvBlockDevice,
    /// 最后一次失败的具体原因 (供上层报告)。
    last_error: Option<BlockError>,
}

impl<'a> BlockAdapter<'a> {
    /// 包装一个驱动层的块设备。
    pub fn new(dev: &'a mut dyn DrvBlockDevice) -> Self {
        Self {
            dev,
            last_error: None,
        }
    }

    /// 取最后一次错误 (若有), 并清除它。
    pub fn take_error(&mut self) -> Option<BlockError> {
        self.last_error.take()
    }
}

impl FsBlockDevice for BlockAdapter<'_> {
    unsafe fn read_block(&mut self, blockno: usize, buf: &mut [u8]) -> bool {
        match self.dev.read(blockno as u64, buf) {
            Ok(()) => true,
            Err(e) => {
                self.last_error = Some(e);
                false
            }
        }
    }

    unsafe fn write_block(&mut self, blockno: usize, buf: &[u8]) -> bool {
        match self.dev.write(blockno as u64, buf) {
            Ok(()) => true,
            Err(e) => {
                self.last_error = Some(e);
                false
            }
        }
    }
}