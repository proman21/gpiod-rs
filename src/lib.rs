#![doc = include_str!("../README.md")]

use std::{
    fmt, fs,
    fs::{File, OpenOptions},
    io::Read,
    ops::Deref,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::{AsRawFd, FromRawFd},
    },
    path::{Path, PathBuf},
};

use gpiod_core::{invalid_input, major, minor, Internal, Result};

pub use gpiod_core::{
    Active, Bias, BitId, ChipInfo, Direction, Drive, Edge, EdgeDetect, Event, Input, LineId,
    LineInfo, Options, Output, Values, ValuesInfo, ValuesIter,
};

#[cfg(not(feature = "v2"))]
fn read_event(_index: &gpiod_core::LineMap, _file: &mut File) -> Result<Event> {
    todo!();
}

#[cfg(feature = "v2")]
fn read_event(index: &gpiod_core::LineMap, file: &mut File) -> Result<Event> {
    let mut event = gpiod_core::RawEvent::default();

    gpiod_core::check_size(file.read(event.as_mut())?, &event)?;

    event.as_event(index)
}

/// Direction trait
pub trait DirectionType: gpiod_core::DirectionType {
    type Lines;

    fn lines(info: Internal<ValuesInfo>, file: File) -> Self::Lines;
}

/// The interface for getting the values of GPIO lines configured for input
///
/// Use [Chip::request_lines] with [Options::input] to configure specific GPIO lines for input.
pub struct Inputs {
    info: Internal<ValuesInfo>,
    // wrap file to call close on drop
    file: File,
}

impl DirectionType for Input {
    type Lines = Inputs;

    fn lines(info: Internal<ValuesInfo>, file: File) -> Self::Lines {
        Self::Lines { info, file }
    }
}

impl Deref for Inputs {
    type Target = ValuesInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

impl Inputs {
    /// Get the value of GPIO lines
    ///
    /// The values can only be read if the lines have previously been requested as inputs
    /// using the [Chip::request_lines] method with [Options::input].
    pub fn get_values<T: From<Values>>(&self) -> Result<T> {
        self.info.get_values(self.file.as_raw_fd()).map(From::from)
    }

    /// Read GPIO events
    pub fn read_event(&mut self) -> Result<Event> {
        read_event(self.info.index(), &mut self.file)
    }
}

/// The interface for setting the values of GPIO lines configured for output
///
/// Use [Chip::request_lines] with [Options::output] to configure specific GPIO lines for output.
///
/// The values also can be read.
/// Specifically this may be useful to get actual value when lines driven as open drain or source.
pub struct Outputs {
    info: Internal<ValuesInfo>,
    // wrap file to call close on drop
    file: File,
}

impl DirectionType for Output {
    type Lines = Outputs;

    fn lines(info: Internal<ValuesInfo>, file: File) -> Self::Lines {
        Self::Lines { info, file }
    }
}

impl Deref for Outputs {
    type Target = ValuesInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

impl Iterator for Inputs {
    type Item = Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.read_event())
    }
}

impl Outputs {
    /// Get the value of GPIO lines
    pub fn get_values<T: From<Values>>(&self) -> Result<T> {
        self.info.get_values(self.file.as_raw_fd()).map(From::from)
    }

    /// Set the value of GPIO lines
    ///
    /// The value can only be set if the lines have previously been requested as outputs
    /// using the [Chip::request_lines] with [Options::output].
    pub fn set_values(&self, values: impl Into<Values>) -> Result<()> {
        self.info.set_values(self.file.as_raw_fd(), values.into())
    }
}

/// A Linux chardev GPIO chip interface
///
/// It can be used to get information about the chip and lines and
/// to request GPIO lines that can be used as inputs or outputs.
pub struct Chip {
    info: Internal<ChipInfo>,
    // wrap file to call close on drop
    file: File,
}

impl Deref for Chip {
    type Target = ChipInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

impl fmt::Display for Chip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.info.fmt(f)
    }
}

impl Chip {
    /// Create a new GPIO chip interface using path
    pub fn new(path: impl AsRef<Path>) -> Result<Chip> {
        let path = path.as_ref();

        let file = OpenOptions::new().read(true).write(true).open(path)?;

        Chip::check_device(path)?;

        Ok(Chip {
            info: Internal::<ChipInfo>::from_fd(file.as_raw_fd())?,
            file,
        })
    }

    /// List all found chips
    pub fn list_devices() -> Result<Vec<PathBuf>> {
        Ok(fs::read_dir("/dev")?
            .filter_map(Result::ok)
            .map(|ent| ent.path())
            .filter(|path| Self::check_device(path).is_ok())
            .collect())
    }

    fn check_device(path: &Path) -> Result<()> {
        let metadata = fs::symlink_metadata(&path)?;

        /* Is it a character device? */
        if !metadata.file_type().is_char_device() {
            return Err(invalid_input("File is not character device"));
        }

        let rdev = metadata.rdev();

        /* Is the device associated with the GPIO subsystem? */
        if fs::canonicalize(format!(
            "/sys/dev/char/{}:{}/subsystem",
            major(rdev),
            minor(rdev)
        ))? != Path::new("/sys/bus/gpio")
        {
            return Err(invalid_input("Character device is not a GPIO"));
        }

        Ok(())
    }

    /// Request the info of a specific GPIO line.
    pub fn line_info(&self, line: LineId) -> Result<LineInfo> {
        self.info.line_info(self.file.as_raw_fd(), line)
    }

    /// Request the GPIO chip to configure the lines passed as argument as inputs or outputs
    ///
    /// Calling this operation is a precondition to being able to set the state of the GPIO lines.
    /// All the lines passed in one request must share the configured options such as active state, edge detect, GPIO bias, output drive and consumer string.
    /// The state of lines configured as outputs can also be read using the [Outputs::get_values] method.
    pub fn request_lines<Direction: DirectionType>(
        &self,
        options: Options<Direction, impl AsRef<[LineId]>, impl AsRef<str>>,
    ) -> Result<Direction::Lines> {
        let (info, fd) = self.info.request_lines(self.file.as_raw_fd(), options)?;

        let file = unsafe { File::from_raw_fd(fd) };

        Ok(Direction::lines(info, file))
    }
}
