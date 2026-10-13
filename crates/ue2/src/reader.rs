use crate::Error;

/// Little-endian cursor over a package buffer with UE2 primitive readers.
#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if n > self.remaining() {
            return Err(Error::Eof { pos: self.pos, want: n });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<(), Error> {
        self.bytes(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    /// UE1/UE2 variable-length signed integer (FCompactIndex).
    pub fn compact(&mut self) -> Result<i32, Error> {
        let b0 = self.u8()?;
        let neg = b0 & 0x80 != 0;
        let mut v = (b0 & 0x3F) as i32;
        if b0 & 0x40 != 0 {
            let mut shift = 6;
            loop {
                let b = self.u8()?;
                v |= ((b & 0x7F) as i32) << shift;
                shift += 7;
                if b & 0x80 == 0 || shift > 27 {
                    break;
                }
            }
        }
        Ok(if neg { -v } else { v })
    }

    /// FString: compact length, positive = ANSI (with NUL), negative = UTF-16.
    pub fn fstring(&mut self) -> Result<String, Error> {
        let len = self.compact()?;
        if len >= 0 {
            let raw = self.bytes(len as usize)?;
            let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
            Ok(raw.iter().map(|&b| char::from(b)).collect())
        } else {
            let n = (-len) as usize;
            let raw = self.bytes(n * 2)?;
            let units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            Ok(String::from_utf16_lossy(&units))
        }
    }
}
