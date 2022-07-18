#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "doc-cfg", feature(doc_cfg))]

#[cfg(all(feature = "tokio", feature = "async-std"))]
compile_error!("Both 'tokio' and 'async-std' features cannot be used simultaneously.");

mod raw;
mod types;
mod utils;

use std::{
    collections::HashMap,
    fmt,
    fs::{canonicalize, read_dir, symlink_metadata, File, OpenOptions},
    io,
    io::Read,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::{FromRawFd, RawFd},
        prelude::*,
    },
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub use types::{
    Active, Bias, BitId, Direction, Drive, Edge, EdgeDetect, Event, LineId, Values, ValuesIter,
};
use utils::*;

macro_rules! unsafe_call {
    ($res:expr) => {
        unsafe { $res }.map_err(io::Error::from)
    };
}

struct LineValues {
    chip_name: String,
    offset: Vec<LineId>,
    index: HashMap<LineId, BitId>,
    file: File,
}

impl LineValues {
    fn new(chip_name: &str, offset: &[LineId], fd: RawFd) -> Self {
        let chip_name = chip_name.into();
        let offset = offset.to_owned();
        let file = unsafe { File::from_raw_fd(fd) };
        let index = offset
            .iter()
            .copied()
            .enumerate()
            .map(|(index, line)| (line, index as _))
            .collect();
        Self {
            chip_name,
            offset,
            index,
            file,
        }
    }

    fn get_values<T: From<Values>>(&self) -> io::Result<T> {
        let mut output_data = Values::default();

        #[cfg(not(feature = "v2"))]
        {
            let mut data = raw::v1::GpioHandleData::default();

            unsafe_call!(raw::v1::gpio_get_line_values(
                self.file.as_raw_fd(),
                &mut data
            ))?;

            for index in 0..self.offset.len() {
                output_data.set(index as _, data.values[index] != 0);
            }
        }

        #[cfg(feature = "v2")]
        {
            unsafe_call!(raw::v2::gpio_line_get_values(
                self.file.as_raw_fd(),
                // it's safe because data layout is same
                &mut output_data as *mut _ as *mut _
            ))?;
        }

        Ok(output_data.into())
    }

    fn set_values(&self, values: impl Into<Values>) -> io::Result<()> {
        let values = values.into();

        #[cfg(not(feature = "v2"))]
        {
            let mut data = raw::v1::GpioHandleData::default();

            for index in 0..self.offset.len() {
                data.values[index] = values.get(index as _).unwrap_or(false) as _;
            }

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
                &mut values as *mut _ as *mut _
            ))?;
        }

        Ok(())
    }

    fn line_bit(&self, line: LineId) -> Option<BitId> {
        self.index.get(&line).copied()
    }

    #[cfg(not(feature = "v2"))]
    fn make_event(&self, line: BitId, event: raw::v1::GpioEventData) -> io::Result<Event> {
        let edge = match event.id {
            raw::v1::GPIOEVENT_EVENT_RISING_EDGE => Edge::Rising,
            raw::v1::GPIOEVENT_EVENT_FALLING_EDGE => Edge::Falling,
            _ => return Err(invalid_data("Unknown edge")),
        };

        let time = SystemTime::UNIX_EPOCH + Duration::from_nanos(event.timestamp);

        Ok(Event { line, edge, time })
    }

    #[cfg(feature = "v2")]
    fn make_event(&self, event: raw::v2::GpioLineEvent) -> io::Result<Event> {
        let line = self
            .line_bit(event.offset)
            .ok_or_else(|| invalid_data("Unknown line offset"))?;

        let edge = match event.id {
            raw::v2::GPIO_LINE_EVENT_RISING_EDGE => Edge::Rising,
            raw::v2::GPIO_LINE_EVENT_FALLING_EDGE => Edge::Falling,
            _ => return Err(invalid_data("Unknown edge")),
        };

        let time = SystemTime::UNIX_EPOCH + Duration::from_nanos(event.timestamp_ns);

        Ok(Event { line, edge, time })
    }

    fn read_event(&mut self) -> io::Result<Event> {
        #[cfg(not(feature = "v2"))]
        {
            // TODO: Read multiple fds simultaneously via polling
            let mut event = raw::v1::GpioEventData::default();

            check_size(self.file.read(event.as_mut())?, &event)?;

            todo!();
            //self.make_event(line, event)
        }

        #[cfg(feature = "v2")]
        {
            let mut event = raw::v2::GpioLineEvent::default();

            check_size(self.file.read(event.as_mut())?, &event)?;

            self.make_event(event)
        }
    }

    #[cfg(any(feature = "tokio", feature = "async-std"))]
    async fn read_event_async(&mut self) -> io::Result<Event> {
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

            self.make_event(event)
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
    pub fn get_values<T: From<Values>>(&self) -> io::Result<T> {
        self.0.get_values()
    }

    /// Read GPIO events synchronously
    pub fn read_event(&mut self) -> io::Result<Event> {
        self.0.read_event()
    }

    /// Read GPIO events asynchronously
    #[cfg_attr(
        feature = "doc-cfg",
        doc(cfg(any(feature = "tokio", feature = "async-std")))
    )]
    #[cfg(any(feature = "tokio", feature = "async-std"))]
    pub async fn read_event_async(&mut self) -> io::Result<Event> {
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
    pub fn get_values<T: From<Values>>(&self) -> io::Result<T> {
        self.0.get_values()
    }

    /// Set the value of GPIO lines
    ///
    /// The value can only be set if the lines have previously been requested as outputs
    /// using the [Chip::request_output].
    pub fn set_values(&self, values: impl Into<Values>) -> io::Result<()> {
        self.0.set_values(values)
    }

    /// Read GPIO events synchronously
    pub fn read_event(&mut self) -> io::Result<Event> {
        self.0.read_event()
    }

    /// Read GPIO events asynchronously
    #[cfg_attr(
        feature = "doc-cfg",
        doc(cfg(any(feature = "tokio", feature = "async-std")))
    )]
    #[cfg(any(feature = "tokio", feature = "async-std"))]
    pub async fn read_event_async(&mut self) -> io::Result<Event> {
        self.0.read_event_async().await
    }
}

/// The information of a specific GPIO line
///
/// Can be obtained through the [Chip::line_info].
pub struct LineInfo {
    direction: Direction,
    active: Active,
    edge: EdgeDetect,
    used: bool,
    bias: Bias,
    drive: Drive,
    name: String,
    consumer: String,
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

impl LineInfo {
    /// Get direction of line
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Get active state of line
    pub fn active(&self) -> Active {
        self.active
    }

    /// Get edge detect of line
    pub fn edge(&self) -> EdgeDetect {
        self.edge
    }

    /// Get input bias of line
    pub fn bias(&self) -> Bias {
        self.bias
    }

    /// In line configured as pull up input
    pub fn is_pull_up(&self) -> bool {
        matches!(self.bias, Bias::PullUp)
    }

    /// In line configured as pull down input
    pub fn is_pull_down(&self) -> bool {
        matches!(self.bias, Bias::PullDown)
    }

    /// Get output mode of line
    pub fn drive(&self) -> Drive {
        self.drive
    }

    /// Is line used
    pub fn is_used(&self) -> bool {
        self.used
    }

    /// Is line configured as open drain output
    pub fn is_open_drain(&self) -> bool {
        matches!(self.drive, Drive::OpenDrain)
    }

    /// Is line configured as open source output
    pub fn is_open_source(&self) -> bool {
        matches!(self.drive, Drive::OpenSource)
    }

    /// Get line name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get line consumer
    pub fn consumer(&self) -> &str {
        &self.consumer
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
    pub fn new(path: impl AsRef<Path>) -> io::Result<Chip> {
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
    pub async fn new_async(path: impl AsRef<Path>) -> io::Result<Chip> {
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
    pub fn list_devices() -> io::Result<Vec<PathBuf>> {
        Ok(read_dir("/dev")?
            .filter_map(Result::ok)
            .map(|ent| ent.path())
            .filter(|path| Self::check_device(path).is_ok())
            .collect())
    }

    /// List all found chips asynchronously
    #[cfg(any(feature = "tokio", feature = "async-std"))]
    pub async fn list_devices_async() -> io::Result<Vec<PathBuf>> {
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

    fn check_device(path: &Path) -> io::Result<()> {
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
    async fn check_device_async(path: &Path) -> io::Result<()> {
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
    pub fn line_info(&self, line: LineId) -> io::Result<LineInfo> {
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

            let direction = if is_set(info.flags, raw::v1::GPIOLINE_FLAG_IS_OUT) {
                Direction::Output
            } else {
                Direction::Input
            };

            let active = if is_set(info.flags, raw::v1::GPIOLINE_FLAG_ACTIVE_LOW) {
                Active::Low
            } else {
                Active::High
            };

            let edge = EdgeDetect::Disable;

            let used = is_set(info.flags, raw::v1::GPIOLINE_FLAG_KERNEL);

            let bias = match (
                is_set(info.flags, raw::v1::GPIOLINE_FLAG_BIAS_PULL_UP),
                is_set(info.flags, raw::v1::GPIOLINE_FLAG_BIAS_PULL_DOWN),
            ) {
                (true, false) => Bias::PullUp,
                (false, true) => Bias::PullDown,
                _ => Bias::Disable,
            };

            let drive = match (
                is_set(info.flags, raw::v1::GPIOLINE_FLAG_OPEN_DRAIN),
                is_set(info.flags, raw::v1::GPIOLINE_FLAG_OPEN_SOURCE),
            ) {
                (true, false) => Drive::OpenDrain,
                (false, true) => Drive::OpenSource,
                _ => Drive::PushPull,
            };
            let name = safe_get_str(&info.name)?.into();
            let consumer = safe_get_str(&info.consumer)?.into();

            Ok(LineInfo {
                direction,
                active,
                edge,
                used,
                bias,
                drive,
                name,
                consumer,
            })
        }

        #[cfg(feature = "v2")]
        {
            let mut info = raw::v2::GpioLineInfo::default();

            info.offset = line;

            unsafe_call!(raw::v2::gpio_get_line_info(
                self.file.as_raw_fd(),
                &mut info
            ))?;

            let direction = if is_set(info.flags, raw::v2::GPIO_LINE_FLAG_OUTPUT) {
                Direction::Output
            } else {
                Direction::Input
            };

            let active = if is_set(info.flags, raw::v2::GPIO_LINE_FLAG_ACTIVE_LOW) {
                Active::Low
            } else {
                Active::High
            };

            let edge = match (
                is_set(info.flags, raw::v2::GPIO_LINE_FLAG_EDGE_RISING),
                is_set(info.flags, raw::v2::GPIO_LINE_FLAG_EDGE_FALLING),
            ) {
                (true, false) => EdgeDetect::Rising,
                (false, true) => EdgeDetect::Falling,
                (true, true) => EdgeDetect::Both,
                _ => EdgeDetect::Disable,
            };

            let used = is_set(info.flags, raw::v2::GPIO_LINE_FLAG_USED);

            let bias = match (
                is_set(info.flags, raw::v2::GPIO_LINE_FLAG_BIAS_PULL_UP),
                is_set(info.flags, raw::v2::GPIO_LINE_FLAG_BIAS_PULL_DOWN),
            ) {
                (true, false) => Bias::PullUp,
                (false, true) => Bias::PullDown,
                _ => Bias::Disable,
            };

            let drive = match (
                is_set(info.flags, raw::v2::GPIO_LINE_FLAG_OPEN_DRAIN),
                is_set(info.flags, raw::v2::GPIO_LINE_FLAG_OPEN_SOURCE),
            ) {
                (true, false) => Drive::OpenDrain,
                (false, true) => Drive::OpenSource,
                _ => Drive::PushPull,
            };
            let name = safe_get_str(&info.name)?.into();
            let consumer = safe_get_str(&info.consumer)?.into();

            Ok(LineInfo {
                direction,
                active,
                edge,
                used,
                bias,
                drive,
                name,
                consumer,
            })
        }
    }

    /// Request the GPIO chip to configure the lines passed as argument as outputs
    ///
    /// Calling this operation is a precondition to being able to set the state of the GPIO lines.
    /// All the lines passed in one request must share the output mode and the active state.
    /// The state of lines configured as outputs can also be read using the [Outputs::get_values] method.
    pub fn request_output(
        &self,
        lines: impl AsRef<[LineId]>,
        active: Active,
        edge: EdgeDetect,
        bias: Bias,
        drive: Drive,
        label: impl AsRef<str>,
    ) -> io::Result<Outputs> {
        let line_offsets = lines.as_ref();

        #[cfg(not(feature = "v2"))]
        let fd = {
            let mut request = raw::v1::GpioHandleRequest::default();

            check_len(line_offsets, &request.line_offsets)?;

            request.lines = line_offsets.len() as _;

            request.line_offsets.copy_from_slice(line_offsets);

            request.flags |= raw::v1::GPIOHANDLE_REQUEST_OUTPUT;

            if matches!(active, Active::Low) {
                request.flags |= raw::v1::GPIOHANDLE_REQUEST_ACTIVE_LOW;
            }

            // TODO: edge detection

            match bias {
                Bias::PullUp => request.flags |= raw::v1::GPIOHANDLE_REQUEST_BIAS_PULL_UP,
                Bias::PullDown => request.flags |= raw::v1::GPIOHANDLE_REQUEST_BIAS_PULL_DOWN,
                Bias::Disable => request.flags |= raw::v1::GPIOHANDLE_REQUEST_BIAS_DISABLE,
            }

            match drive {
                Drive::OpenDrain => request.flags |= raw::v1::GPIOHANDLE_REQUEST_OPEN_DRAIN,
                Drive::OpenSource => request.flags |= raw::v1::GPIOHANDLE_REQUEST_OPEN_SOURCE,
                _ => (),
            }

            safe_set_str(&mut request.consumer_label, label.as_ref())?;

            unsafe_call!(raw::v1::gpio_get_line_handle(
                self.file.as_raw_fd(),
                &mut request,
            ))?;

            request.fd
        };

        #[cfg(feature = "v2")]
        let fd = {
            let mut request = raw::v2::GpioLineRequest::default();

            check_len(line_offsets, &request.offsets)?;

            request.num_lines = line_offsets.len() as _;

            request.offsets.copy_from_slice(line_offsets);

            request.config.flags |= raw::v2::GPIO_LINE_FLAG_OUTPUT;

            if matches!(active, Active::Low) {
                request.config.flags |= raw::v2::GPIO_LINE_FLAG_ACTIVE_LOW;
            }

            match edge {
                EdgeDetect::Rising => request.config.flags |= raw::v2::GPIO_LINE_FLAG_EDGE_RISING,
                EdgeDetect::Falling => request.config.flags |= raw::v2::GPIO_LINE_FLAG_EDGE_FALLING,
                EdgeDetect::Both => request.config.flags |= raw::v2::GPIO_LINE_FLAG_EDGE_BOTH,
                _ => {}
            }

            match bias {
                Bias::PullUp => request.config.flags |= raw::v2::GPIO_LINE_FLAG_BIAS_PULL_UP,
                Bias::PullDown => request.config.flags |= raw::v2::GPIO_LINE_FLAG_BIAS_PULL_DOWN,
                Bias::Disable => request.config.flags |= raw::v2::GPIO_LINE_FLAG_BIAS_DISABLED,
            }

            match drive {
                Drive::OpenDrain => request.config.flags |= raw::v2::GPIO_LINE_FLAG_OPEN_DRAIN,
                Drive::OpenSource => request.config.flags |= raw::v2::GPIO_LINE_FLAG_OPEN_SOURCE,
                _ => (),
            };

            safe_set_str(&mut request.consumer, label.as_ref())?;

            unsafe_call!(raw::v2::gpio_get_line(self.file.as_raw_fd(), &mut request))?;

            request.fd
        };

        Ok(Outputs(LineValues::new(&self.name, line_offsets, fd)))
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
    ) -> io::Result<Inputs> {
        let line_offsets = lines.as_ref();

        #[cfg(not(feature = "v2"))]
        let fd = {
            let mut request = raw::v1::GpioHandleRequest::default();

            check_len(line_offsets, &request.line_offsets)?;

            request.lines = line_offsets.len() as _;

            request.line_offsets.copy_from_slice(line_offsets);

            request.flags |= raw::v1::GPIOHANDLE_REQUEST_INPUT;

            if matches!(active, Active::Low) {
                request.flags |= raw::v1::GPIOHANDLE_REQUEST_ACTIVE_LOW;
            }

            // TODO: edge detection

            match bias {
                Bias::PullUp => request.flags |= raw::v1::GPIOHANDLE_REQUEST_BIAS_PULL_UP,
                Bias::PullDown => request.flags |= raw::v1::GPIOHANDLE_REQUEST_BIAS_PULL_DOWN,
                Bias::Disable => request.flags |= raw::v1::GPIOHANDLE_REQUEST_BIAS_DISABLE,
            }

            safe_set_str(&mut request.consumer_label, label.as_ref())?;

            unsafe_call!(raw::v1::gpio_get_line_handle(
                self.file.as_raw_fd(),
                &mut request,
            ))?;

            request.fd
        };

        #[cfg(feature = "v2")]
        let fd = {
            let mut request = raw::v2::GpioLineRequest::default();

            check_len(line_offsets, &request.offsets)?;

            request.num_lines = line_offsets.len() as _;

            request.offsets.copy_from_slice(line_offsets);

            request.config.flags |= raw::v2::GPIO_LINE_FLAG_INPUT;

            if matches!(active, Active::Low) {
                request.config.flags |= raw::v2::GPIO_LINE_FLAG_ACTIVE_LOW;
            }

            match edge {
                EdgeDetect::Rising => request.config.flags |= raw::v2::GPIO_LINE_FLAG_EDGE_RISING,
                EdgeDetect::Falling => request.config.flags |= raw::v2::GPIO_LINE_FLAG_EDGE_FALLING,
                EdgeDetect::Both => request.config.flags |= raw::v2::GPIO_LINE_FLAG_EDGE_BOTH,
                _ => {}
            }

            match bias {
                Bias::PullUp => request.config.flags |= raw::v2::GPIO_LINE_FLAG_BIAS_PULL_UP,
                Bias::PullDown => request.config.flags |= raw::v2::GPIO_LINE_FLAG_BIAS_PULL_DOWN,
                Bias::Disable => request.config.flags |= raw::v2::GPIO_LINE_FLAG_BIAS_DISABLED,
            }

            safe_set_str(&mut request.consumer, label.as_ref())?;

            unsafe_call!(raw::v2::gpio_get_line(self.file.as_raw_fd(), &mut request))?;

            request.fd
        };

        Ok(Inputs(LineValues::new(&self.name, line_offsets, fd)))
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
