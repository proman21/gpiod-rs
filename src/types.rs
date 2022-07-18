use std::{fmt, str, time::SystemTime};

/// Line offset
pub type LineId = u32;

/// Bit offset
pub type BitId = u8;

/// Line values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct Values {
    /// Logic values of lines
    pub bits: u64,
    /// Mask of lines to get or set
    pub mask: u64,
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
                    values.bits as _
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

impl Values {
    /// Get the value of specific bit
    ///
    /// If bit is out of range (0..64) or not masked then None will be returned.
    pub fn get(&self, bit: BitId) -> Option<bool> {
        if bit > 64 {
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
        if bit > 64 {
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
        let max = (64 - (self.bits & self.mask).leading_zeros() as u8).max(1);
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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("0b").unwrap_or(s);
        let mut i = s.len();
        if i > 64 {
            return Err(());
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
                _ => return Err(()),
            }
        }
        Ok(r)
    }
}

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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "i" | "in" | "input" => Self::Input,
            "o" | "out" | "output" => Self::Output,
            _ => return Err(()),
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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "l" | "lo" | "low" | "active-low" => Self::Low,
            "h" | "hi" | "high" | "active-high" => Self::High,
            _ => return Err(()),
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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "r" | "rise" | "rising" => Self::Rising,
            "f" | "fall" | "falling" => Self::Falling,
            _ => return Err(()),
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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "d" | "disable" => Self::Disable,
            "r" | "rise" | "rising" => Self::Rising,
            "f" | "fall" | "falling" => Self::Falling,
            "b" | "both" | "rise-fall" | "rising-falling" => Self::Both,
            _ => return Err(()),
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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "d" | "disable" => Self::Disable,
            "pu" | "pull-up" => Self::PullUp,
            "pd" | "pull-down" => Self::PullUp,
            _ => return Err(()),
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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "pp" | "push-pull" => Self::PushPull,
            "od" | "open-drain" => Self::OpenDrain,
            "os" | "open-source" => Self::OpenSource,
            _ => return Err(()),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn format_values() {
        assert_eq!(Values::from(0b1000u8).to_string(), "0b1000");

        assert_eq!(
            Values {
                bits: 0b0011,
                mask: 0b0111,
            }
            .to_string(),
            "0b11"
        );

        assert_eq!(
            Values {
                bits: 0b11000,
                mask: 0b00011,
            }
            .to_string(),
            "0b0"
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
                bits: u64::MAX,
                mask: u64::MAX,
            }
        );

        assert_eq!(
            "0b0000000000000000000000000000000000000000000000000000000000000000"
                .parse::<Values>()
                .unwrap(),
            Values {
                bits: 0,
                mask: u64::MAX,
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
