mod bucket;
mod list;

use wie_util::Result;

use crate::{
    ArmCore,
    core::{HEAP_BASE, HEAP_SIZE},
};

use self::{
    bucket::{BUCKET_MAX, BucketAllocator},
    list::ListAllocator,
};

pub struct Allocator;

impl Allocator {
    pub fn init(core: &mut ArmCore) -> Result<()> {
        core.map(HEAP_BASE, HEAP_SIZE)?;

        ListAllocator::init(core, HEAP_BASE, HEAP_SIZE / 2)?;
        BucketAllocator::init(core, HEAP_BASE + HEAP_SIZE / 2, HEAP_SIZE / 2)?;

        Ok(())
    }

    pub fn alloc(core: &mut ArmCore, size: u32) -> Result<u32> {
        if size > BUCKET_MAX as _ {
            ListAllocator::alloc(core, HEAP_BASE, HEAP_SIZE / 2, size)
        } else {
            BucketAllocator::alloc(core, HEAP_BASE + HEAP_SIZE / 2, size)
        }
    }

    pub fn free(core: &mut ArmCore, address: u32, size: u32) -> Result<()> {
        if size > BUCKET_MAX as _ {
            ListAllocator::free(core, address)
        } else {
            BucketAllocator::free(core, HEAP_BASE + HEAP_SIZE / 2, address, size)
        }
    }

    pub fn allocation_size(core: &ArmCore, address: u32) -> Result<u32> {
        if address < HEAP_BASE + HEAP_SIZE / 2 {
            ListAllocator::allocation_size(core, address)
        } else {
            BucketAllocator::allocation_size(HEAP_BASE + HEAP_SIZE / 2, address)
        }
    }

    pub fn free_unsized(core: &mut ArmCore, address: u32) -> Result<()> {
        if address < HEAP_BASE + HEAP_SIZE / 2 {
            ListAllocator::free(core, address)
        } else {
            BucketAllocator::free_unsized(core, HEAP_BASE + HEAP_SIZE / 2, address)
        }
    }
}


#[cfg(test)]
mod tests {
    use wie_util::Result;

    use crate::ArmCore;

    use super::Allocator;

    #[test]
    fn allocation_size_recovers_bucket_and_list_capacity() -> Result<()> {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core)?;

        let bucket = Allocator::alloc(&mut core, 20)?;
        assert_eq!(Allocator::allocation_size(&core, bucket)?, 32);

        let list = Allocator::alloc(&mut core, 513)?;
        assert_eq!(Allocator::allocation_size(&core, list)?, 516);

        Ok(())
    }

    #[test]
    fn free_unsized_recovers_bucket_and_list_allocations() -> Result<()> {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core)?;

        // 20 bytes is served by the 32-byte bucket. Unsized free must recover
        // that class from the returned address rather than from a caller size.
        let bucket = Allocator::alloc(&mut core, 20)?;
        Allocator::free_unsized(&mut core, bucket)?;
        let bucket_again = Allocator::alloc(&mut core, 20)?;
        assert_eq!(bucket_again, bucket);

        // 513 bytes crosses BUCKET_MAX and is served by the list allocator.
        // The heap-half address alone must select the list free path.
        let list = Allocator::alloc(&mut core, 513)?;
        Allocator::free_unsized(&mut core, list)?;
        let list_again = Allocator::alloc(&mut core, 513)?;
        assert_eq!(list_again, list);

        Ok(())
    }
}
