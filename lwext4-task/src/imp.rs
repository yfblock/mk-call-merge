use blk_task::{BlockIface, BLOCK_SIZE, BLK};
use lwext4_rust::KernelDevOp;

/// 块设备适配器：将 blk-task 的 BlockIface 适配为 lwext4 的 KernelDevOp
pub struct Ext4Disk {
    block_id: usize,
    offset: usize,
}

impl KernelDevOp for Ext4Disk {
    type DevType = Self;

    fn write(dev: &mut Self::DevType, buf: &[u8]) -> Result<usize, i32> {
        dev.write_one(buf)
    }

    fn read(dev: &mut Self::DevType, buf: &mut [u8]) -> Result<usize, i32> {
        dev.read_one(buf)
    }

    fn seek(dev: &mut Self::DevType, off: i64, whence: i32) -> Result<i64, i32> {
        let size = BLK.capacity() as i64;
        let new_pos = match whence as u32 {
            0 => Some(off),                                                    // SEEK_SET
            1 => dev.position().checked_add_signed(off).map(|v| v as i64),     // SEEK_CUR
            2 => size.checked_add(off),                                        // SEEK_END
            _ => {
                log::error!("invalid seek() whence: {}", whence);
                Some(off)
            }
        }
        .ok_or(-1i32)?;

        if new_pos as u64 > size as u64 {
            return Err(-1i32);
        }

        dev.set_position(new_pos as u64);
        Ok(new_pos)
    }

    fn flush(_dev: &mut Self::DevType) -> Result<usize, i32>
    where
        Self: Sized,
    {
        Ok(0)
    }
}

impl Ext4Disk {
    pub fn new() -> Self {
        Self {
            block_id: 0,
            offset: 0,
        }
    }

    pub fn position(&self) -> u64 {
        (self.block_id * BLOCK_SIZE + self.offset) as u64
    }

    pub fn set_position(&mut self, pos: u64) {
        self.block_id = pos as usize / BLOCK_SIZE;
        self.offset = pos as usize % BLOCK_SIZE;
    }

    fn read_one(&mut self, buf: &mut [u8]) -> Result<usize, i32> {
        assert_eq!(buf.len() % BLOCK_SIZE, 0);
        assert_eq!(self.offset, 0);

        let block_num = buf.len() / BLOCK_SIZE;
        for i in 0..block_num {
            let offset = i * BLOCK_SIZE;
            BLK.read_block(self.block_id + i, &mut buf[offset..offset + BLOCK_SIZE]);
        }
        self.set_position(self.position() + buf.len() as u64);
        Ok(buf.len())
    }

    fn write_one(&mut self, buf: &[u8]) -> Result<usize, i32> {
        assert_eq!(buf.len() % BLOCK_SIZE, 0);
        assert_eq!(self.offset, 0);

        let block_num = buf.len() / BLOCK_SIZE;
        for i in 0..block_num {
            let offset = i * BLOCK_SIZE;
            BLK.write_block(self.block_id + i, &buf[offset..offset + BLOCK_SIZE]);
        }
        self.set_position(self.position() + buf.len() as u64);
        Ok(buf.len())
    }
}
