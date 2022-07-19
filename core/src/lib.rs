#![doc = include_str!("../README.md")]

#[cfg(not(target_os = "linux"))]
compile_error!("This crate support Linux only");

mod iop;
mod raw;
mod types;
mod utils;

use std::{fmt, os::unix::io::RawFd};

pub use iop::RawEvent;
pub use std::{
    io::{Error, Result},
    time::SystemTime as Time,
};
pub use types::{
    Active, Bias, BitId, Direction, Drive, Edge, EdgeDetect, Event, LineId, LineInfo, LineMap,
    Values, ValuesIter,
};
pub use utils::*;

macro_rules! unsafe_call {
    ($res:expr) => {
        unsafe { $res }.map_err(Error::from)
    };
}

pub struct LineValues {
    pub chip_name: String,
    pub consumer: String,
    pub lines: Vec<LineId>,
    pub index: LineMap,
}

impl fmt::Display for LineValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}] {:?}", self.chip_name, self.consumer, self.lines)
    }
}

impl LineValues {
    pub fn new(chip_name: &str, consumer: &str, lines: &[LineId]) -> Self {
        let chip_name = chip_name.into();
        let consumer = consumer.into();
        let index = LineMap::new(lines);
        let lines = lines.to_owned();
        Self {
            chip_name,
            consumer,
            lines,
            index,
        }
    }

    pub fn get_values(&self, fd: RawFd) -> Result<Values> {
        #[cfg(not(feature = "v2"))]
        let values = {
            let mut data = raw::v1::GpioHandleData::default();

            unsafe_call!(raw::v1::gpio_get_line_values(fd, &mut data))?;

            data.as_values(self.lines.len())
        };

        #[cfg(feature = "v2")]
        let values = {
            let mut values = Values::default();

            unsafe_call!(raw::v2::gpio_line_get_values(fd, values.as_mut(),))?;

            values
        };

        Ok(values)
    }

    pub fn set_values(&self, fd: RawFd, values: Values) -> Result<()> {
        #[cfg(not(feature = "v2"))]
        {
            let mut data = raw::v1::GpioHandleData::from_values(self.lines.len(), &values);

            unsafe_call!(raw::v1::gpio_set_line_values(fd, &mut data))?;
        }

        #[cfg(feature = "v2")]
        {
            let mut values = values;

            unsafe_call!(raw::v2::gpio_line_set_values(fd, values.as_mut(),))?;
        }

        Ok(())
    }
}

/// A Linux chardev GPIO chip interface
///
/// It can be used to get information about the chip and lines and
/// to request GPIO lines that can be used as inputs or outputs.
pub struct ChipInfo {
    pub name: String,
    pub label: String,
    pub num_lines: LineId,
}

impl fmt::Display for ChipInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] ({} lines)",
            self.name, self.label, self.num_lines
        )
    }
}

impl ChipInfo {
    pub fn from_fd(fd: RawFd) -> Result<Self> {
        let mut info = raw::GpioChipInfo::default();

        unsafe_call!(raw::gpio_get_chip_info(fd, &mut info))?;

        Ok(Self {
            name: safe_get_str(&info.name)?.into(),
            label: safe_get_str(&info.label)?.into(),
            num_lines: info.lines,
        })
    }

    /// Request the info of a specific GPIO line.
    pub fn line_info(&self, fd: RawFd, line: LineId) -> Result<LineInfo> {
        #[cfg(not(feature = "v2"))]
        {
            let mut info = raw::v1::GpioLineInfo {
                line_offset: line,
                ..Default::default()
            };

            unsafe_call!(raw::v1::gpio_get_line_info(fd, &mut info))?;

            info.as_info()
        }

        #[cfg(feature = "v2")]
        {
            let mut info = raw::v2::GpioLineInfo::default();

            info.offset = line;

            unsafe_call!(raw::v2::gpio_get_line_info(fd, &mut info))?;

            info.as_info()
        }
    }

    /// Request the GPIO chip to configure the lines passed as argument as outputs
    ///
    /// Calling this operation is a precondition to being able to set the state of the GPIO lines.
    /// All the lines passed in one request must share the output mode and the active state.
    #[allow(clippy::too_many_arguments)]
    pub fn request_lines(
        &self,
        fd: RawFd,
        lines: &[LineId],
        direction: Direction,
        active: Active,
        edge: Option<EdgeDetect>,
        bias: Option<Bias>,
        drive: Option<Drive>,
        values: Option<Values>,
        label: &str,
    ) -> Result<(LineValues, RawFd)> {
        #[cfg(not(feature = "v2"))]
        let fd = {
            let mut request =
                raw::v1::GpioHandleRequest::new(lines, direction, active, bias, drive, label)?;

            // TODO: edge detection

            unsafe_call!(raw::v1::gpio_get_line_handle(fd, &mut request))?;

            if let Some(values) = values {
                let mut data = raw::v1::GpioHandleData::from_values(lines.len(), &values);

                unsafe_call!(raw::v1::gpio_set_line_values(fd, &mut data))?;
            }

            request.fd
        };

        #[cfg(feature = "v2")]
        let fd = {
            let mut request = raw::v2::GpioLineRequest::new(
                lines, direction, active, edge, bias, drive, values, label,
            )?;

            unsafe_call!(raw::v2::gpio_get_line(fd, &mut request))?;

            request.fd
        };

        Ok((LineValues::new(&self.name, label, lines), fd))
    }
}
