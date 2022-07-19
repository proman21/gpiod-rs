#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "doc-cfg", feature(doc_cfg))]

#[cfg(all(feature = "tokio", feature = "async-std"))]
compile_error!("Both 'tokio' and 'async-std' features cannot be used simultaneously.");

mod iop;
mod raw;
mod types;
mod utils;

use std::{
    fmt,
    fs::{canonicalize, read_dir, symlink_metadata, File, OpenOptions},
    io::Read,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::{FromRawFd, RawFd},
        prelude::*,
    },
    path::{Path, PathBuf},
};

pub(crate) use std::{
    io::{Error, Result},
    time::SystemTime as Time,
};
pub use types::{
    Active, Bias, BitId, Direction, Drive, Edge, EdgeDetect, Event, LineId, LineInfo, LineMap,
    Values, ValuesIter,
};
use utils::*;

macro_rules! unsafe_call {
    ($res:expr) => {
        unsafe { $res }.map_err(Error::from)
    };
}

struct LineValues {
    chip_name: String,
    offset: Vec<LineId>,
    index: LineMap,
    // wrap file to call close on drop
    file: File,
}

impl LineValues {
    fn new(chip_name: &str, offset: &[LineId], fd: RawFd) -> Self {
        let chip_name = chip_name.into();
        let index = LineMap::new(offset);
        let offset = offset.to_owned();
        let file = unsafe { File::from_raw_fd(fd) };
        Self {
            chip_name,
            offset,
            index,
            file,
        }
    }

    fn get_values<T: From<Values>>(&self) -> Result<T> {
        #[cfg(not(feature = "v2"))]
        let values = {
            let mut data = raw::v1::GpioHandleData::default();

            unsafe_call!(raw::v1::gpio_get_line_values(
                self.file.as_raw_fd(),
                &mut data
            ))?;

            data.as_values(self.offset.len())
        };

        #[cfg(feature = "v2")]
        let values = {
            let mut values = Values::default();

            unsafe_call!(raw::v2::gpio_line_get_values(
                self.file.as_raw_fd(),
                values.as_mut(),
            ))?;

            values
        };

        Ok(values.into())
    }

    fn set_values(&self, values: impl Into<Values>) -> Result<()> {
        let values = values.into();

        #[cfg(not(feature = "v2"))]
        {
            let mut data = raw::v1::GpioHandleData::from_values(self.offset.len(), &values);

            unsafe_call!(raw::v1::gpio_set_line_values(
                self.file.as_raw_fd(),
                &mut data
            ))?;
        }

        #[cfg(feature = "v2")]
        {
            let mut values = values;

            unsafe_call!(raw::v2::gpio_line_set_values(
                self.file.as_raw_fd(),
                values.as_mut(),
            ))?;
        }

        Ok(())
    }

    fn read_event(&mut self) -> Result<Event> {
        #[cfg(not(feature = "v2"))]
        {
            // TODO: Read multiple fds simultaneously via polling
            let mut event = raw::v1::GpioEventData::default();

            todo!();
            //Ok(event.as_event(line))
        }

        #[cfg(feature = "v2")]
        {
            let mut event = raw::v2::GpioLineEvent::default();

            check_size(self.file.read(event.as_mut())?, &event)?;

            event.as_event(&self.index)
        }
    }

    #[cfg(any(feature = "tokio", feature = "async-std"))]
    async fn read_event_async(&mut self) -> Result<Event> {
        #[cfg(not(feature = "v2"))]
        {
            todo!();
        }

        #[cfg(feature = "v2")]
        {
            #[cfg(feature = "tokio")]
            use tokio::io::AsyncReadExt;

            #[cfg(feature = "async-std")]
            use async_std::io::ReadExt;

            let mut event = raw::v2::GpioLineEvent::default();

            #[cfg(feature = "tokio")]
            let mut file = unsafe { tokio::fs::File::from_raw_fd(self.file.as_raw_fd()) };

            #[cfg(feature = "async-std")]
            let mut file = unsafe { async_std::fs::File::from_raw_fd(self.file.as_raw_fd()) };

            let res = file.read(event.as_mut()).await;

            // bypass close syscall
            core::mem::forget(file);

            check_size(res?, &event)?;

            event.as_event(&self.index)
        }
    }
}

/// The interface for getting the values of GPIO lines configured for input
///
/// Use [Chip::request_input] to configure specific GPIO lines for input.
pub struct Inputs(LineValues);

impl AsRef<File> for Inputs {
    fn as_ref(&self) -> &File {
        &self.0.file
    }
}

impl Inputs {
    /// Get associated chip name
    pub fn chip_name(&self) -> &str {
        &self.0.chip_name
    }

    /// Get offsets of requested lines
    pub fn lines(&self) -> &[LineId] {
        &self.0.offset
    }

    /// Get the value of GPIO lines
    ///
    /// The values can only be read if the lines have previously been requested as either inputs
    /// using the [Chip::request_input] method, or outputs using the [Chip::request_output].
    pub fn get_values<T: From<Values>>(&self) -> Result<T> {
        self.0.get_values()
    }

    /// Read GPIO events synchronously
    pub fn read_event(&mut self) -> Result<Event> {
        self.0.read_event()
    }

    /// Read GPIO events asynchronously
    #[cfg_attr(
        feature = "doc-cfg",
        doc(cfg(any(feature = "tokio", feature = "async-std")))
    )]
    #[cfg(any(feature = "tokio", feature = "async-std"))]
    pub async fn read_event_async(&mut self) -> Result<Event> {
        self.0.read_event_async().await
    }
}

/// The interface for setting the values of GPIO lines configured for output
///
/// Use [Chip::request_output] to configure specific GPIO lines for output.
///
/// The values also can be read.
/// Specifically this may be useful to get actual value when lines driven as open drain or source.
pub struct Outputs(LineValues);

impl AsRef<File> for Outputs {
    fn as_ref(&self) -> &File {
        &self.0.file
    }
}

impl Outputs {
    /// Get associated chip name
    pub fn chip_name(&self) -> &str {
        &self.0.chip_name
    }

    /// Get offsets of requested lines
    pub fn lines(&self) -> &[LineId] {
        &self.0.offset
    }

    /// Get the value of GPIO lines
    ///
    /// The values can only be read if the lines have previously been requested as either inputs
    /// using the [Chip::request_input] method, or outputs using the [Chip::request_output].
    pub fn get_values<T: From<Values>>(&self) -> Result<T> {
        self.0.get_values()
    }

    /// Set the value of GPIO lines
    ///
    /// The value can only be set if the lines have previously been requested as outputs
    /// using the [Chip::request_output].
    pub fn set_values(&self, values: impl Into<Values>) -> Result<()> {
        self.0.set_values(values)
    }

    /// Read GPIO events synchronously
    pub fn read_event(&mut self) -> Result<Event> {
        self.0.read_event()
    }

    /// Read GPIO events asynchronously
    #[cfg_attr(
        feature = "doc-cfg",
        doc(cfg(any(feature = "tokio", feature = "async-std")))
    )]
    #[cfg(any(feature = "tokio", feature = "async-std"))]
    pub async fn read_event_async(&mut self) -> Result<Event> {
        self.0.read_event_async().await
    }
}

/// A Linux chardev GPIO chip interface
///
/// It can be used to get information about the chip and lines and
/// to request GPIO lines that can be used as inputs or outputs.
pub struct Chip {
    name: String,
    label: String,
    num_lines: LineId,
    // wrap file to call close on drop
    file: File,
}

impl fmt::Display for Chip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] ({} lines)",
            self.name, self.label, self.num_lines
        )
    }
}

impl Chip {
    /// Create a new GPIO chip interface using path
    pub fn new(path: impl AsRef<Path>) -> Result<Chip> {
        let path = path.as_ref();

        let dev = OpenOptions::new().read(true).write(true).open(path)?;

        Chip::check_device(path)?;

        let mut info = raw::GpioChipInfo::default();

        unsafe_call!(raw::gpio_get_chip_info(dev.as_raw_fd(), &mut info))?;

        Ok(Chip {
            name: safe_get_str(&info.name)?.into(),
            label: safe_get_str(&info.label)?.into(),
            num_lines: info.lines,
            file: dev,
        })
    }

    /// Create a new GPIO chip interface using path asynchronously
    #[cfg(any(feature = "tokio", feature = "async-std"))]
    pub async fn new_async(path: impl AsRef<Path>) -> Result<Chip> {
        let path = path.as_ref();

        let dev = OpenOptions::new().read(true).write(true).open(path)?;

        Chip::check_device_async(path).await?;

        let mut info = raw::GpioChipInfo::default();

        unsafe_call!(raw::gpio_get_chip_info(dev.as_raw_fd(), &mut info))?;

        Ok(Chip {
            name: safe_get_str(&info.name)?.into(),
            label: safe_get_str(&info.label)?.into(),
            num_lines: info.lines,
            file: dev,
        })
    }

    /// List all found chips
    pub fn list_devices() -> Result<Vec<PathBuf>> {
        Ok(read_dir("/dev")?
            .filter_map(Result::ok)
            .map(|ent| ent.path())
            .filter(|path| Self::check_device(path).is_ok())
            .collect())
    }

    /// List all found chips asynchronously
    #[cfg(any(feature = "tokio", feature = "async-std"))]
    pub async fn list_devices_async() -> Result<Vec<PathBuf>> {
        #[cfg(feature = "tokio")]
        use tokio::fs::read_dir;

        #[cfg(feature = "async-std")]
        use async_std::{fs::read_dir, stream::StreamExt};

        let mut devices = Vec::new();
        let mut dir = read_dir("/dev").await?;

        #[cfg(feature = "tokio")]
        while let Some(ent) = dir.next_entry().await? {
            let path = ent.path();
            if Self::check_device_async(&path).await.is_ok() {
                devices.push(path.into());
            }
        }

        #[cfg(feature = "async-std")]
        while let Some(ent) = dir.next().await {
            let path = ent?.path();
            if Self::check_device_async(path.as_ref()).await.is_ok() {
                devices.push(path.into());
            }
        }

        Ok(devices)
    }

    fn check_device(path: &Path) -> Result<()> {
        let metadata = symlink_metadata(&path)?;

        /* Is it a character device? */
        if !metadata.file_type().is_char_device() {
            return Err(invalid_input("File is not character device"));
        }

        let rdev = metadata.rdev();

        /* Is the device associated with the GPIO subsystem? */
        if canonicalize(format!(
            "/sys/dev/char/{}:{}/subsystem",
            major(rdev),
            minor(rdev)
        ))? != Path::new("/sys/bus/gpio")
        {
            return Err(invalid_input("Character device is not a GPIO"));
        }

        Ok(())
    }

    #[cfg(any(feature = "tokio", feature = "async-std"))]
    async fn check_device_async(path: &Path) -> Result<()> {
        #[cfg(feature = "tokio")]
        use tokio::fs::{canonicalize, symlink_metadata};

        #[cfg(feature = "async-std")]
        use async_std::{
            fs::{canonicalize, symlink_metadata},
            path::Path,
        };

        let metadata = symlink_metadata(&path).await?;

        /* Is it a character device? */
        if !metadata.file_type().is_char_device() {
            return Err(invalid_input("File is not character device"));
        }

        let rdev = metadata.rdev();

        /* Is the device associated with the GPIO subsystem? */
        if canonicalize(format!(
            "/sys/dev/char/{}:{}/subsystem",
            major(rdev),
            minor(rdev)
        ))
        .await?
            != Path::new("/sys/bus/gpio")
        {
            return Err(invalid_input("Character device is not a GPIO"));
        }

        Ok(())
    }

    /// Request the info of a specific GPIO line.
    pub fn line_info(&self, line: LineId) -> Result<LineInfo> {
        #[cfg(not(feature = "v2"))]
        {
            let mut info = raw::v1::GpioLineInfo {
                line_offset: line,
                ..Default::default()
            };

            unsafe_call!(raw::v1::gpio_get_line_info(
                self.file.as_raw_fd(),
                &mut info
            ))?;

            info.as_info()
        }

        #[cfg(feature = "v2")]
        {
            let mut info = raw::v2::GpioLineInfo::default();

            info.offset = line;

            unsafe_call!(raw::v2::gpio_get_line_info(
                self.file.as_raw_fd(),
                &mut info
            ))?;

            info.as_info()
        }
    }

    /// Request the GPIO chip to configure the lines passed as argument as outputs
    ///
    /// Calling this operation is a precondition to being able to set the state of the GPIO lines.
    /// All the lines passed in one request must share the output mode and the active state.
    /// The state of lines configured as outputs can also be read using the [Outputs::get_values] method.
    #[allow(clippy::too_many_arguments)]
    pub fn request_output(
        &self,
        lines: impl AsRef<[LineId]>,
        active: Active,
        edge: EdgeDetect,
        bias: Bias,
        drive: Drive,
        values: Option<Values>,
        label: impl AsRef<str>,
    ) -> Result<Outputs> {
        let lines = lines.as_ref();
        let label = label.as_ref();

        #[cfg(not(feature = "v2"))]
        let fd = {
            let mut request = raw::v1::GpioHandleRequest::new(
                lines,
                Direction::Output,
                active,
                Some(bias),
                Some(drive),
                label,
            )?;

            // TODO: edge detection

            unsafe_call!(raw::v1::gpio_get_line_handle(
                self.file.as_raw_fd(),
                &mut request,
            ))?;

            if let Some(values) = values {
                let mut data = raw::v1::GpioHandleData::from_values(lines.len(), &values);

                unsafe_call!(raw::v1::gpio_set_line_values(
                    self.file.as_raw_fd(),
                    &mut data
                ))?;
            }

            request.fd
        };

        #[cfg(feature = "v2")]
        let fd = {
            let mut request = raw::v2::GpioLineRequest::new(
                lines,
                Direction::Output,
                active,
                Some(edge),
                Some(bias),
                Some(drive),
                values,
                label,
            )?;

            unsafe_call!(raw::v2::gpio_get_line(self.file.as_raw_fd(), &mut request))?;

            request.fd
        };

        Ok(Outputs(LineValues::new(&self.name, lines, fd)))
    }

    /// Request the GPIO chip to configure the lines passed as argument as inputs
    ///
    /// Calling this operation is a precondition to being able to read the state of the GPIO lines.
    pub fn request_input(
        &self,
        lines: impl AsRef<[LineId]>,
        active: Active,
        edge: EdgeDetect,
        bias: Bias,
        label: impl AsRef<str>,
    ) -> Result<Inputs> {
        let lines = lines.as_ref();
        let label = label.as_ref();

        #[cfg(not(feature = "v2"))]
        let fd = {
            let mut request = raw::v1::GpioHandleRequest::new(
                lines,
                Direction::Input,
                active,
                Some(bias),
                None,
                label,
            )?;

            // TODO: edge detection

            unsafe_call!(raw::v1::gpio_get_line_handle(
                self.file.as_raw_fd(),
                &mut request,
            ))?;

            request.fd
        };

        #[cfg(feature = "v2")]
        let fd = {
            let mut request = raw::v2::GpioLineRequest::new(
                lines,
                Direction::Input,
                active,
                Some(edge),
                Some(bias),
                None,
                None,
                label,
            )?;

            unsafe_call!(raw::v2::gpio_get_line(self.file.as_raw_fd(), &mut request))?;

            request.fd
        };

        Ok(Inputs(LineValues::new(&self.name, lines, fd)))
    }

    /// Get the GPIO chip name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the GPIO chip label
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Get the total number of lines of the GPIO chip
    pub fn num_lines(&self) -> LineId {
        self.num_lines
    }
}
