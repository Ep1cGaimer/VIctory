//! Changes to title code made in memory once the image is mapped (never on
//! disk), each located by a per-build offset and checked against a
//! byte-pattern signature before it is written.

/// One change: at `rva`, the bytes `expect` must be found; `write` then
/// goes at `rva + at`.
#[derive(Debug, Clone)]
pub struct CodePatch {
    /// Our name for it, for the log.
    pub name: &'static str,
    pub rva: usize,
    pub expect: &'static [u8],
    pub at: usize,
    pub write: Vec<u8>,
}

/// The slots of the lock-free pool the job threads take from by default
/// (`VICTORY_TITLE_POOL_SLOTS`, 0 leaves the title's own size).
pub const POOL_SLOTS: u32 = 65_536;

/// The pool's slot indices are 17 bits wide, 0x1fffe meaning empty.
const POOL_SLOTS_MAX: u32 = 0x1_fffd;

/// What the title needs changed in `build` (its BLAKE3, hex) to run here.
///
/// Series 1.0.2.1 sizes a pool of 224-byte job slots from its pool-size
/// table (entry `0x5411e15c`) and stops on an `int3` when the pool is
/// empty. At the frame rates we reach, the slots in use climb from about
/// 8,500 to over 21,000 a few minutes into the world (they are given back
/// later) and the pool runs dry. The size lookup's call is replaced by
/// `mov eax, slots`; 65,536 slots take 15 MB.
pub fn patches(build: &str, pool_slots: u32) -> Vec<CodePatch> {
    let mut patches = Vec::new();
    if build.eq_ignore_ascii_case(crate::KNOWN_BUILDS[0].blake3) && pool_slots > 0 {
        let slots = pool_slots.min(POOL_SLOTS_MAX);
        let mut write = vec![0xb8];
        write.extend_from_slice(&slots.to_le_bytes());
        patches.push(CodePatch {
            name: "job slot pool size",
            rva: 0x5ad_8e4d,
            // mov r8d, 0x5411e15c; mov r9d, 1; mov rcx, [rax+8];
            // mov edx, [rax+0x10]; call (the size lookup)
            expect: &[
                0x41, 0xb8, 0x5c, 0xe1, 0x11, 0x54, 0x41, 0xb9, 0x01, 0x00, 0x00, 0x00, 0x48, 0x8b,
                0x48, 0x08, 0x8b, 0x50, 0x10, 0xe8,
            ],
            at: 0x13,
            write,
        });
    }
    patches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_is_replaced_by_a_mov_of_the_same_length() {
        let p = patches(crate::KNOWN_BUILDS[0].blake3, 65_536);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].write, [0xb8, 0x00, 0x00, 0x01, 0x00]);
        assert_eq!(p[0].at + p[0].write.len(), p[0].expect.len() + 4);
        assert!(patches(crate::KNOWN_BUILDS[0].blake3, 0).is_empty());
        assert!(patches("00", 65_536).is_empty());
        assert_eq!(patches(crate::KNOWN_BUILDS[0].blake3, u32::MAX)[0].write[1..], 0x1_fffd_u32.to_le_bytes());
    }
}
