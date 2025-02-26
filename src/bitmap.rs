pub struct WordMap {
    limbs: Vec<u32>,
    len: usize,
    bitlen: usize,
}
impl WordMap {
    pub fn new(len: usize, bitlen: usize) -> Self {
        assert!([1, 2, 4].contains(&bitlen));

        let mut alloc = (len * bitlen) / 32;
        if (len * bitlen) % 32 > 0 {
            alloc += 1;
        }

        Self {
            limbs: vec![0u32; alloc],
            len,
            bitlen,
        }
    }
    pub fn set(&mut self, offset: usize, value: u32) -> usize {
        assert!(offset < self.len);
        assert!(value < 1 << self.bitlen);

        let limb = offset / (32 / self.bitlen);
        let pos = offset % (32 / self.bitlen);
        let shift = 32 - pos * self.bitlen - self.bitlen;
        let bitmask: u32 = (0b1111 >> (4 - self.bitlen)) << shift;

        self.limbs[limb] = (self.limbs[limb] & !bitmask) | (value << shift);

        limb
    }
    pub fn get(&self, offset: usize) -> u32 {
        assert!(offset < self.len);

        let limb = offset / (32 / self.bitlen);
        let pos = offset % (32 / self.bitlen);
        let shift = 32 - pos * self.bitlen - self.bitlen;
        let bitmask: u32 = 0b1111 >> (4 - self.bitlen);

        (self.limbs[limb] >> shift) & bitmask
    }
}

#[test]
fn test_wordmap() {
    for bitlen in [1, 2, 4] {
        let mut wm = WordMap::new(65, bitlen);
        let wordsize = 1 << bitlen;
        for i in 0..wm.len {
            wm.set(i, (i as u32 % (1 + wordsize)) % wordsize);
        }
        for i in 0..wm.len {
            assert_eq!(wm.get(i), (i as u32 % (1 + wordsize)) % wordsize);
        }
    }
}
