use crate::{invalid_input, BitId, Error, Result};
use std::{fmt, str};

/// Value bits and mask
pub type Bits = u64;

/// Maximum number of values which can be get or set per time
pub const MAX_VALUES: usize = core::mem::size_of::<Bits>() * 8;

/// Maximum number of bits which can be get or set per time
pub const MAX_BITS: BitId = MAX_VALUES as _;

/// Default values representation
pub type Values = Masked<Bits>;

/// Something that can be used to get GPIO line values
pub trait AsValues {
    //// Number of bits
    fn bits(&self) -> BitId;

    /// Get the value of specific bit identified by offset
    ///
    /// If bit is out of range (0..bits) or not masked then None should be returned.
    fn get(&self, id: BitId) -> Option<bool>;

    /// Copy values to another variable
    fn copy_into<T: AsValuesMut>(&self, other: &mut T) {
        for id in 0..self.bits().min(other.bits()) {
            other.set(id, self.get(id));
        }
    }

    /// Convert to another representation
    fn convert<T: AsValuesMut + Default>(&self) -> T {
        let mut other = T::default();
        self.copy_into(&mut other);
        other
    }
}

/// Something that can be used to get and set GPIO line values
pub trait AsValuesMut: AsValues {
    /// Set the value of specific bit identified by offset
    ///
    /// If bit if out of range (0..bits) then nothing should be set.
    fn set(&mut self, id: BitId, val: Option<bool>);

    /// Change the value of specific bit identified by offset
    ///
    /// If bit if out of range (0..bits) then nothing will be changed.
    fn with(mut self, id: BitId, val: Option<bool>) -> Self
    where
        Self: Sized,
    {
        self.set(id, val);
        self
    }

    /// Copy values to another variable
    fn copy_from<T: AsValues>(&mut self, other: &T) {
        for id in 0..self.bits().min(other.bits()) {
            self.set(id, other.get(id));
        }
    }

    /// Fill values in range
    fn fill<R: Iterator<Item = BitId>>(&mut self, range: R, val: Option<bool>) {
        for id in range {
            self.set(id, val);
        }
    }

    /// Truncate mask
    fn truncate(&mut self, len: BitId) {
        for id in len..self.bits() {
            self.set(id, None);
        }
    }
}

impl<T: AsValues> AsValues for &T {
    fn bits(&self) -> BitId {
        (**self).bits()
    }

    fn get(&self, id: BitId) -> Option<bool> {
        (**self).get(id)
    }
}

impl<T: AsValues> AsValues for &mut T {
    fn bits(&self) -> BitId {
        (**self).bits()
    }

    fn get(&self, id: BitId) -> Option<bool> {
        (**self).get(id)
    }
}

impl<T: AsValuesMut> AsValuesMut for &mut T {
    fn set(&mut self, id: BitId, val: Option<bool>) {
        (**self).set(id, val)
    }
}

/// Line values with mask
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct Masked<Bits> {
    /// Logic values of lines
    pub bits: Bits,

    /// Mask of lines to get or set
    pub mask: Bits,
}

macro_rules! as_values {
    ($($type:ty,)*) => {
        $(
            impl AsValues for $type {
                fn bits(&self) -> BitId {
                    (core::mem::size_of::<$type>() * 8) as _
                }

                fn get(&self, id: BitId) -> Option<bool> {
                    if id >= (core::mem::size_of::<$type>() * 8) as _ {
                        return None;
                    }

                    Some(self & (1 << id) != 0)
                }
            }

            impl AsValuesMut for $type {
                fn set(&mut self, id: BitId, val: Option<bool>) {
                    if id >= (core::mem::size_of::<$type>() * 8) as _ {
                        return;
                    }

                    let mask = (1 as $type) << id;

                    if let Some(true) = val {
                        *self |= mask;
                    } else {
                        *self &= !mask;
                    }
                }
            }

            impl AsValues for Masked<$type> {
                fn bits(&self) -> BitId {
                    (core::mem::size_of::<$type>() * 8) as _
                }

                fn get(&self, id: BitId) -> Option<bool> {
                    if id >= (core::mem::size_of::<$type>() * 8) as _ {
                        return None;
                    }

                    let mask = (1 as $type) << id;

                    if self.mask & mask == 0 {
                        return None;
                    }

                    Some(self.bits & mask != 0)
                }
            }

            impl AsValuesMut for Masked<$type> {
                fn set(&mut self, id: BitId, val: Option<bool>) {
                    if id >= (core::mem::size_of::<$type>() * 8) as _ {
                        return;
                    }

                    let mask = (1 as $type) << id;

                    if let Some(val) = val {
                        self.mask |= mask;

                        if val {
                            self.bits |= mask;
                        } else {
                            self.bits &= !mask;
                        }
                    } else {
                        let mask = !mask;

                        self.mask &= mask;
                        self.bits &= mask;
                    }
                }
            }

            impl From<$type> for Masked<$type> {
                fn from(bits: $type) -> Self {
                    Self {
                        bits: bits as _,
                        mask: <$type>::MAX as _,
                    }
                }
            }

            impl From<Masked<$type>> for $type {
                fn from(values: Masked<$type>) -> Self {
                    (values.bits & values.mask) as _
                }
            }

            impl fmt::Binary for Masked<$type> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    use fmt::Write;

                    let max = (core::mem::size_of::<$type>() * 8) as BitId;
                    let len = (max - (self.mask & self.bits).leading_zeros() as BitId).max(1);
                    let fill = f.width().map(|width| {
                        let width = if f.alternate() {
                            width - 2
                        } else {
                            width
                        };
                        if width > len as _ {
                            width - len as usize
                        } else {
                            0
                        }
                    }).unwrap_or(0);
                    let (fill_before, fill_after) = match f.align() {
                        Some(fmt::Alignment::Left) => (0, fill),
                        Some(fmt::Alignment::Right) | None => (fill, 0),
                        Some(fmt::Alignment::Center) => (fill - fill / 2, fill / 2),
                    };
                    let fill_char = f.fill();
                    if f.alternate() {
                        f.write_str("0b")?;
                    }
                    for _ in 0..fill_before {
                        f.write_char(fill_char)?;
                    }
                    for i in (0..len).rev() {
                        f.write_char(match self.get(i) {
                            Some(true) => '1',
                            Some(false) => '0',
                            None => 'x',
                        })?;
                    }
                    for _ in 0..fill_after {
                        f.write_char(fill_char)?;
                    }
                    Ok(())
                }
            }

            impl fmt::Display for Masked<$type> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    fmt::Binary::fmt(self, f)
                }
            }

            impl str::FromStr for Masked<$type> {
                type Err = Error;

                fn from_str(s: &str) -> Result<Self> {
                    let s = s.strip_prefix("0b").unwrap_or(s);
                    let mut i = s.len() as BitId;
                    if i > (core::mem::size_of::<$type>() * 8) as _ {
                        return Err(invalid_input("Too many line values"));
                    }
                    let mut r = Self::default();
                    for c in s.chars() {
                        i -= 1;
                        match c {
                            '1' => {
                                let b = 1 << i;
                                r.bits |= b;
                                r.mask |= b;
                            }
                            '0' => {
                                let b = 1 << i;
                                r.mask |= b;
                            }
                            'x' => {}
                            _ => return Err(invalid_input("Unexpected char in line value")),
                        }
                    }
                    Ok(r)
                }
            }

        )*
    };
}

as_values! {
    u8,
    u16,
    u32,
    u64,
}

impl AsValues for [bool] {
    fn bits(&self) -> BitId {
        self.len() as _
    }

    fn get(&self, id: BitId) -> Option<bool> {
        if id >= self.len() as _ {
            return None;
        }

        Some(self[id as usize])
    }
}

impl AsValuesMut for [bool] {
    fn set(&mut self, id: BitId, val: Option<bool>) {
        if id >= self.len() as _ {
            return;
        }

        if let Some(val) = val {
            self[id as usize] = val;
        }
    }
}

impl AsValues for Vec<bool> {
    fn bits(&self) -> BitId {
        self.len() as _
    }

    fn get(&self, id: BitId) -> Option<bool> {
        if id >= self.len() as _ {
            return None;
        }

        Some(self[id as usize])
    }
}

impl AsValuesMut for Vec<bool> {
    fn set(&mut self, id: BitId, val: Option<bool>) {
        if id >= self.len() as _ {
            return;
        }

        if let Some(val) = val {
            self[id as usize] = val;
        }
    }
}

impl<const LEN: usize> AsValues for [bool; LEN] {
    fn bits(&self) -> BitId {
        LEN as _
    }

    fn get(&self, id: BitId) -> Option<bool> {
        if id >= LEN as _ {
            return None;
        }

        Some(self[id as usize])
    }
}

impl<const LEN: usize> AsValuesMut for [bool; LEN] {
    fn set(&mut self, id: BitId, val: Option<bool>) {
        if id >= LEN as _ {
            return;
        }

        if let Some(val) = val {
            self[id as usize] = val;
        }
    }
}

impl AsValues for [Option<bool>] {
    fn bits(&self) -> BitId {
        self.len() as _
    }

    fn get(&self, id: BitId) -> Option<bool> {
        if id >= self.len() as _ {
            return None;
        }

        self[id as usize]
    }
}

impl AsValuesMut for [Option<bool>] {
    fn set(&mut self, id: BitId, val: Option<bool>) {
        if id >= self.len() as _ {
            return;
        }

        self[id as usize] = val;
    }
}

impl AsValues for Vec<Option<bool>> {
    fn bits(&self) -> BitId {
        self.len() as _
    }

    fn get(&self, id: BitId) -> Option<bool> {
        if id >= self.len() as _ {
            return None;
        }

        self[id as usize]
    }
}

impl AsValuesMut for Vec<Option<bool>> {
    fn set(&mut self, id: BitId, val: Option<bool>) {
        if id >= self.len() as _ {
            return;
        }

        self[id as usize] = val;
    }
}

impl<const LEN: usize> AsValues for [Option<bool>; LEN] {
    fn bits(&self) -> BitId {
        LEN as _
    }

    fn get(&self, id: BitId) -> Option<bool> {
        if id >= LEN as _ {
            return None;
        }

        self[id as usize]
    }
}

impl<const LEN: usize> AsValuesMut for [Option<bool>; LEN] {
    fn set(&mut self, id: BitId, val: Option<bool>) {
        if id >= LEN as _ {
            return;
        }

        self[id as usize] = val;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn format_masked() {
        assert_eq!(Masked::from(0b1000u8).to_string(), "1000");

        assert_eq!(
            Values {
                bits: 0b1000,
                mask: 0b1111,
            }
            .to_string(),
            "1000"
        );

        assert_eq!(
            Values {
                bits: 0b0011,
                mask: 0b0111,
            }
            .to_string(),
            "11"
        );

        assert_eq!(
            Values {
                bits: 0b0011,
                mask: 0b1111,
            }
            .to_string(),
            "11"
        );

        assert_eq!(
            Values {
                bits: 0b11000,
                mask: 0b00011,
            }
            .to_string(),
            "0"
        );

        assert_eq!(
            Values {
                bits: 0b100001,
                mask: 0b110011,
            }
            .to_string(),
            "10xx01"
        );
    }

    #[test]
    fn format_masked_advanced() {
        assert_eq!(format!("{:#}", Masked::from(0b1000u8)), "0b1000");

        assert_eq!(format!("{:#08b}", 0b1000u8), "0b001000");

        //assert_eq!(format!("{:#08b}", Masked::from(0b1000u8)), "0b001000");

        assert_eq!(format!("{:11}", Masked::from(0b1000u8)), "       1000");

        assert_eq!(format!("{:-<11}", Masked::from(0b1000u8)), "1000-------");

        assert_eq!(format!("{:->11}", Masked::from(0b1000u8)), "-------1000");

        assert_eq!(format!("{:-^11}", Masked::from(0b1000u8)), "----1000---");
    }

    #[test]
    fn parse_masked() {
        assert_eq!(
            "0110".parse::<Values>().unwrap(),
            Values {
                bits: 0b0110,
                mask: 0b1111,
            }
        );

        assert_eq!(
            "00110".parse::<Values>().unwrap(),
            Values {
                bits: 0b00110,
                mask: 0b11111,
            }
        );

        assert_eq!(
            "0b10101".parse::<Values>().unwrap(),
            Values {
                bits: 0b10101,
                mask: 0b11111,
            }
        );

        assert_eq!(
            "1x10x".parse::<Values>().unwrap(),
            Values {
                bits: 0b10100,
                mask: 0b10110,
            }
        );

        assert_eq!(
            "xx0x010".parse::<Values>().unwrap(),
            Values {
                bits: 0b00010,
                mask: 0b10111,
            }
        );

        assert_eq!(
            "0bxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                .parse::<Values>()
                .unwrap(),
            Values::default()
        );

        assert_eq!(
            "0b1111111111111111111111111111111111111111111111111111111111111111"
                .parse::<Values>()
                .unwrap(),
            Values {
                bits: Bits::MAX,
                mask: Bits::MAX,
            }
        );

        assert_eq!(
            "0b0000000000000000000000000000000000000000000000000000000000000000"
                .parse::<Values>()
                .unwrap(),
            Values {
                bits: 0,
                mask: Bits::MAX,
            }
        );

        assert!(
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                .parse::<Values>()
                .is_err()
        );

        assert!("0b10xy".parse::<Values>().is_err());
    }
}
