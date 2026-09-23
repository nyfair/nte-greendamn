pub const CAP: usize = 640;

pub struct Buf {
    data: [u8; CAP],
    len: usize,
}

impl Buf {
    pub const fn new() -> Self {
        Buf {
            data: [0; CAP],
            len: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }

    pub fn push_byte(&mut self, b: u8) {
        if self.len < CAP {
            self.data[self.len] = b;
            self.len += 1;
        }
    }

    pub fn push_bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.push_byte(x);
        }
    }

    pub fn push_str(&mut self, s: &str) {
        self.push_bytes(s.as_bytes());
    }

    pub fn push_u64(&mut self, mut v: u64) {
        if v == 0 {
            self.push_byte(b'0');
            return;
        }
        let mut tmp = [0u8; 20];
        let mut n = 0;
        while v > 0 {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
        while n > 0 {
            n -= 1;
            self.push_byte(tmp[n]);
        }
    }

    pub fn push_u64_pad(&mut self, v: u64, width: usize) {
        let mut tmp = [0u8; 20];
        let mut n = 0;
        let mut x = v;
        if x == 0 {
            tmp[0] = b'0';
            n = 1;
        }
        while x > 0 {
            tmp[n] = b'0' + (x % 10) as u8;
            x /= 10;
            n += 1;
        }
        let mut pad = width.saturating_sub(n);
        while pad > 0 {
            self.push_byte(b'0');
            pad -= 1;
        }
        while n > 0 {
            n -= 1;
            self.push_byte(tmp[n]);
        }
    }

    pub fn push_hex(&mut self, v: u64, width: usize) {
        let mut tmp = [0u8; 16];
        let mut n = 0;
        let mut x = v;
        if x == 0 {
            tmp[0] = b'0';
            n = 1;
        }
        while x > 0 {
            let d = (x & 0xF) as u8;
            tmp[n] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
            x >>= 4;
            n += 1;
        }
        let mut pad = width.saturating_sub(n);
        while pad > 0 {
            self.push_byte(b'0');
            pad -= 1;
        }
        while n > 0 {
            n -= 1;
            self.push_byte(tmp[n]);
        }
    }
}
