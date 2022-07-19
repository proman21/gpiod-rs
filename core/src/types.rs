use crate::{utils::*, Error, Result};
use std::{fmt, str, time::SystemTime};

/// Line offset
pub type LineId = u32;

/// Bit offset
pub type BitId = u8;

/// Value bits and mask
pub type Bits = u64;

/// Line offset to bit offset mapping
#[derive(Debug, Clone)]
pub struct LineMap {
    map: Vec<BitId>,
}

impl LineMap {
    const NOT_LINE: BitId = Values::MAX as _;

    /// Create line map
    pub fn new(lines: &[LineId]) -> Self {
        let mut map: Vec<BitId> = (0..=lines.iter().max().copied().unwrap_or(0))
            .map(|_| Self::NOT_LINE)
            .collect();
        for i in 0..lines.len() {
            map[lines[i] as usize] = i as _;
        }
        Self { map }
    }

    /// Get bit position by line offset
    pub fn get(&self, line: LineId) -> Result<BitId> {
        let line = line as usize;
        if line < self.map.len() {
            let val = self.map[line];
            if val != Self::NOT_LINE {
                return Ok(val as _);
            }
        }
        Err(invalid_data("Unknown line offset"))
    }
}

/// The information of a specific GPIO line
pub struct LineInfo {
    /// GPIO line direction
    pub direction: Direction,

    /// GPIO line active state
    pub active: Active,

    /// GPIO line edge detection
    pub edge: EdgeDetect,

    /// GPIO line usage status
    ///
    /// `true` means that kernel uses this line for some purposes.
    pub used: bool,

    /// GPIO line input bias
    pub bias: Bias,

    /// GPIO line output drive mode
    pub drive: Drive,

    /// GPIO line name
    pub name: String,

    /// GPIO line consumer name
    pub consumer: String,
}

impl fmt::Display for LineInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\t {}", self.direction)?;
        if self.used {
            write!(f, "\t Used")?;
        } else {
            write!(f, "\t Unused")?;
        }
        if self.consumer.is_empty() {
            write!(f, "\t Unnamed")?;
        } else {
            write!(f, "\t {}", self.consumer)?;
        }
        write!(f, "\t Active {}", self.active)?;
        if !matches!(self.drive, Drive::PushPull) {
            write!(f, "\t {}", self.drive)?;
        }
        if !matches!(self.edge, EdgeDetect::Disable) {
            write!(f, "\t Edge {}", self.edge)?;
        }
        Ok(())
    }
}

/// Line values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct Values {
    /// Logic values of lines
    pub bits: Bits,

    /// Mask of lines to get or set
    pub mask: Bits,
}

impl Values {
    /// Maximum number of values (bits)
    pub const MAX: usize = core::mem::size_of::<Bits>() * 8;

    /// Get the value of specific bit
    ///
    /// If bit is out of range (0..64) or not masked then None will be returned.
    pub fn get(&self, bit: BitId) -> Option<bool> {
        if bit > Self::MAX as _ {
            return None;
        }

        let mask = 1 << bit;

        if (self.mask & mask) == 0 {
            return None;
        }

        Some(self.bits & mask != 0)
    }

    /// Set the value of specific bit and mask it
    ///
    /// If bit if out of range (0..64) then nothing will be set.
    pub fn set(&mut self, bit: BitId, val: bool) {
        if bit > Self::MAX as _ {
            return;
        }

        let mask = 1 << bit;

        self.mask |= mask;

        if val {
            self.bits |= mask;
        } else {
            self.bits &= !mask;
        }
    }
}

impl fmt::Display for Values {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let max = (Self::MAX as BitId - self.mask.leading_zeros() as BitId).max(1);
        "0b".fmt(f)?;
        for i in (0..max).rev() {
            match self.get(i) {
                Some(true) => '1',
                Some(false) => '0',
                None => 'x',
            }
            .fmt(f)?;
        }
        Ok(())
    }
}

impl str::FromStr for Values {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.strip_prefix("0b").unwrap_or(s);
        let mut i = s.len();
        if i > Self::MAX {
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

macro_rules! values_conv {
    ($($type:ty,)*) => {
        $(
            impl From<$type> for Values {
                fn from(bits: $type) -> Self {
                    Self {
                        bits: bits as _,
                        mask: <$type>::MAX as _,
                    }
                }
            }

            impl From<Values> for $type {
                fn from(values: Values) -> Self {
                    (values.bits & values.mask) as _
                }
            }
        )*
    };
}

values_conv! {
    u8,
    u16,
    u32,
    u64,
}

impl Extend<bool> for Values {
    fn extend<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = bool>,
    {
        let mut vacant = self.mask.leading_zeros();

        for value in iter {
            if vacant > 0 {
                self.bits = (self.bits << 1) | if value { 1 } else { 0 };
                self.mask = (self.mask << 1) | 1;
            }
            vacant -= 1;
        }
    }
}

impl core::iter::FromIterator<bool> for Values {
    fn from_iter<I: IntoIterator<Item = bool>>(bits: I) -> Self {
        let mut values = Self::default();
        let mut i = Self::MAX;
        for bit in bits {
            i -= 1;
            let mask = 1 << i;
            if bit {
                values.bits |= mask;
            }
            values.mask |= mask;
            if i == 0 {
                break;
            }
        }
        if i > 0 {
            values.bits >>= i;
            values.mask >>= i;
        }
        values
    }
}

impl core::iter::IntoIterator for Values {
    type Item = bool;
    type IntoIter = ValuesIter;

    fn into_iter(self) -> Self::IntoIter {
        ValuesIter {
            bits: self.bits,
            i: Self::MAX as BitId - self.mask.leading_zeros() as BitId,
        }
    }
}

/// Iterator over line values
#[derive(Debug, Clone, Copy)]
pub struct ValuesIter {
    bits: Bits,
    i: BitId,
}

impl core::iter::Iterator for ValuesIter {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == 0 {
            None
        } else {
            self.i -= 1;
            Some(self.bits & (1 << self.i) != 0)
        }
    }
}

/*
impl<T: AsRef<[bool]>> TryFrom<T> for Values {
    fn from(bits: T) -> Self {
        let bits = bits.as_ref();
        let mut values = Self::default();
        for i in 0..bits.len().min(Self::MAX) {
            values.set(i as _, bits[63 - i]);
        }
        values
    }
}

impl From<Values> for Vec<bool> {
    fn from(values: Values) -> Self {
        let mut i = Self::MAX as BitId - values.mask.leading_zeros() as BitId;
        let mut bits = Vec::with_capacity(i as _);
        let raw = values.bits & values.mask;
        while i > 0 {
            bits.push(raw & (1 << i) != 0);
            i -= 1;
        }
        bits
    }
}
*/

/// Direction of a GPIO line
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Direction {
    /// Line acts as input (default)
    Input,
    /// Line acts as output
    Output,
}

impl Default for Direction {
    fn default() -> Self {
        Self::Input
    }
}

impl AsRef<str> for Direction {
    fn as_ref(&self) -> &str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl str::FromStr for Direction {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "i" | "in" | "input" => Self::Input,
            "o" | "out" | "output" => Self::Output,
            _ => return Err(invalid_input("Not recognized direction")),
        })
    }
}

/// Active state condition of a line
///
/// If active state of line is **high** then physical and logical levels is same.
/// Otherwise if it is **low** then physical level will be inverted from logical.
///
/// Also this may be treated as polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Active {
    /// Active level is low
    Low,
    /// Active level is high (default)
    High,
}

impl Default for Active {
    fn default() -> Self {
        Self::High
    }
}

impl AsRef<str> for Active {
    fn as_ref(&self) -> &str {
        match self {
            Self::Low => "low",
            Self::High => "high",
        }
    }
}

impl fmt::Display for Active {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl str::FromStr for Active {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "l" | "lo" | "low" | "active-low" => Self::Low,
            "h" | "hi" | "high" | "active-high" => Self::High,
            _ => return Err(invalid_input("Not recognized active state")),
        })
    }
}

/// Signal edge or level transition of a GPIO line
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Edge {
    /// Rising edge detected
    Rising,
    /// Falling edge detected
    Falling,
}

impl AsRef<str> for Edge {
    fn as_ref(&self) -> &str {
        match self {
            Self::Rising => "rising",
            Self::Falling => "falling",
        }
    }
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl str::FromStr for Edge {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "r" | "rise" | "rising" => Self::Rising,
            "f" | "fall" | "falling" => Self::Falling,
            _ => return Err(invalid_input("Not recognized edge")),
        })
    }
}

/// Signal edge detection event
#[derive(Debug, Clone, Copy)]
pub struct Event {
    /// GPIO line where edge detected
    pub line: BitId,
    /// Detected edge or level transition
    pub edge: Edge,
    /// Time when edge actually detected
    pub time: SystemTime,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        '#'.fmt(f)?;
        self.line.fmt(f)?;
        ' '.fmt(f)?;
        self.edge.fmt(f)
        //' '.fmt(f)?;
        //(&self.time as &dyn fmt::Debug).fmt(f)
    }
}

/// Edge detection setting for GPIO line
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EdgeDetect {
    /// Detection disabled (default)
    Disable,
    /// Detect rising edge only
    Rising,
    /// Detect falling edge only
    Falling,
    /// Detect both rising and falling edges
    Both,
}

impl Default for EdgeDetect {
    fn default() -> Self {
        Self::Disable
    }
}

impl AsRef<str> for EdgeDetect {
    fn as_ref(&self) -> &str {
        match self {
            Self::Disable => "disable",
            Self::Rising => "rising",
            Self::Falling => "falling",
            Self::Both => "both",
        }
    }
}

impl fmt::Display for EdgeDetect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl str::FromStr for EdgeDetect {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "d" | "disable" => Self::Disable,
            "r" | "rise" | "rising" => Self::Rising,
            "f" | "fall" | "falling" => Self::Falling,
            "b" | "both" | "rise-fall" | "rising-falling" => Self::Both,
            _ => return Err(invalid_input("Not recognized edge-detect")),
        })
    }
}

/// Input bias of a GPIO line
///
/// Sometimes GPIO lines shall be pulled to up (power rail) or down (ground)
/// through resistor to avoid floating level on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Bias {
    /// Disabled bias (default)
    Disable,
    /// Pull line up
    PullUp,
    /// Pull line down
    PullDown,
}

impl Default for Bias {
    fn default() -> Self {
        Self::Disable
    }
}

impl AsRef<str> for Bias {
    fn as_ref(&self) -> &str {
        match self {
            Self::Disable => "disable",
            Self::PullUp => "pull-up",
            Self::PullDown => "pull-down",
        }
    }
}

impl fmt::Display for Bias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl str::FromStr for Bias {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "d" | "disable" => Self::Disable,
            "pu" | "pull-up" => Self::PullUp,
            "pd" | "pull-down" => Self::PullUp,
            _ => return Err(invalid_input("Not recognized input bias")),
        })
    }
}

/// Output drive mode of a GPIO line
///
/// Usually GPIO lines configured as push-pull but sometimes it required to drive via open drain or source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Drive {
    /// Drive push-pull (default)
    PushPull,
    /// Drive with open-drain
    OpenDrain,
    /// Drive with open-source
    OpenSource,
}

impl Default for Drive {
    fn default() -> Self {
        Self::PushPull
    }
}

impl AsRef<str> for Drive {
    fn as_ref(&self) -> &str {
        match self {
            Self::PushPull => "push-pull",
            Self::OpenDrain => "open-drain",
            Self::OpenSource => "open-source",
        }
    }
}

impl fmt::Display for Drive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

impl str::FromStr for Drive {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "pp" | "push-pull" => Self::PushPull,
            "od" | "open-drain" => Self::OpenDrain,
            "os" | "open-source" => Self::OpenSource,
            _ => return Err(invalid_input("Not recognized output drive")),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn format_values() {
        assert_eq!(Values::from(0b1000u8).to_string(), "0b00001000");

        assert_eq!(
            Values {
                bits: 0b1000,
                mask: 0b1111,
            }
            .to_string(),
            "0b1000"
        );

        assert_eq!(
            Values {
                bits: 0b0011,
                mask: 0b0111,
            }
            .to_string(),
            "0b011"
        );

        assert_eq!(
            Values {
                bits: 0b0011,
                mask: 0b1111,
            }
            .to_string(),
            "0b0011"
        );

        assert_eq!(
            Values {
                bits: 0b11000,
                mask: 0b00011,
            }
            .to_string(),
            "0b00"
        );

        assert_eq!(
            Values {
                bits: 0b100001,
                mask: 0b110011,
            }
            .to_string(),
            "0b10xx01"
        );
    }

    #[test]
    fn parse_values() {
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

    #[test]
    fn values_extend() {
        let mut v = Values {
            bits: 0b101,
            mask: 0b111,
        };
        v.extend([false, true, true, false, true]);

        assert_eq!(
            v,
            Values {
                bits: 0b10101101,
                mask: 0b11111111,
            }
        );
    }

    #[test]
    fn values_from_iter() {
        assert_eq!(
            Values::from_iter([true, false, false, true]),
            Values {
                bits: 0b1001,
                mask: 0b1111,
            }
        );

        assert_eq!(
            Values::from_iter([false, false, true, true, true, false]),
            Values {
                bits: 0b001110,
                mask: 0b111111,
            }
        );
    }

    #[test]
    fn values_into_iter() {
        assert_eq!(
            Values {
                bits: 0b1001,
                mask: 0b1111,
            }
            .into_iter()
            .collect::<Vec<_>>(),
            [true, false, false, true]
        );

        assert_eq!(
            Values {
                bits: 0b001110,
                mask: 0b111111,
            }
            .into_iter()
            .collect::<Vec<_>>(),
            [false, false, true, true, true, false]
        );
    }
}
