#![doc = include_str!("../README.md")]

use std::{
    fmt,
    ops::Deref,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::{AsRawFd, FromRawFd, RawFd},
    },
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use gpiod_core::{invalid_input, major, minor, Internal, Result};

pub use gpiod_core::{
    Active, AsValues, AsValuesMut, Bias, BitId, ChipInfo, Direction, Drive, Edge, EdgeDetect,
    Event, Input, LineId, LineInfo, Masked, Options, Output, Values, ValuesInfo, MAX_BITS,
    MAX_VALUES,
};

use async_io::Async;
use async_std::{
    fs,
    fs::OpenOptions,
    io::{Read, ReadExt},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    stream::StreamExt,
    task::spawn_blocking as asyncify,
};

#[doc(hidden)]
pub struct File {
    // use file to call close when drop
    inner: Async<std::fs::File>,
}

impl File {
    pub fn from_fd(fd: RawFd) -> Result<Self> {
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        Ok(Self {
            inner: Async::new(file)?,
        })
    }

    pub fn from_file(file: fs::File) -> Result<Self> {
        let fd = file.as_raw_fd();
        core::mem::forget(file);
        Self::from_fd(fd)
    }
}

impl AsRawFd for File {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl Read for File {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        use std::io::Read;

        match self.inner.poll_readable(cx) {
            Poll::Ready(x) => x,
            Poll::Pending => return Poll::Pending,
        }?;

        Poll::Ready(self.inner.get_ref().read(buf))
    }
}

#[cfg(not(feature = "v2"))]
async fn read_event(_index: &gpiod_core::LineMap, _file: &mut File) -> Result<Event> {
    todo!();
}

#[cfg(feature = "v2")]
async fn read_event(index: &gpiod_core::LineMap, file: &mut File) -> Result<Event> {
    let mut event = gpiod_core::RawEvent::default();

    gpiod_core::check_size(file.read(event.as_mut()).await?, &event)?;

    event.as_event(index)
}

/// Direction trait
pub trait DirectionType: gpiod_core::DirectionType {
    type Lines;

    fn lines(info: Arc<Internal<ValuesInfo>>, file: File) -> Self::Lines;
}

/// The interface for getting the values of GPIO lines configured for input
///
/// Use [Chip::request_lines] with [Options::input] to configure specific GPIO lines for input.
pub struct Inputs {
    info: Arc<Internal<ValuesInfo>>,
    // wrap file to call close on drop
    file: File,
}

impl DirectionType for Input {
    type Lines = Inputs;

    fn lines(info: Arc<Internal<ValuesInfo>>, file: File) -> Self::Lines {
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
    pub async fn get_values<T: AsValuesMut + Send + 'static>(&self, mut values: T) -> Result<T> {
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();
        asyncify(move || info.get_values(fd, &mut values).map(|_| values)).await
    }

    /// Read GPIO events synchronously
    pub async fn read_event(&mut self) -> Result<Event> {
        read_event(self.info.index(), &mut self.file).await
    }
}

/// The interface for setting the values of GPIO lines configured for output
///
/// Use [Chip::request_lines] with [Options::output] to configure specific GPIO lines for output.
///
/// The values also can be read.
/// Specifically this may be useful to get actual value when lines driven as open drain or source.
pub struct Outputs {
    info: Arc<Internal<ValuesInfo>>,
    // wrap file to call close on drop
    file: File,
}

impl DirectionType for Output {
    type Lines = Outputs;

    fn lines(info: Arc<Internal<ValuesInfo>>, file: File) -> Self::Lines {
        Self::Lines { info, file }
    }
}

impl Deref for Outputs {
    type Target = ValuesInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

impl Outputs {
    /// Get the value of GPIO lines
    pub async fn get_values<T: AsValuesMut + Send + 'static>(&self, mut values: T) -> Result<T> {
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();
        asyncify(move || info.get_values(fd, &mut values).map(|_| values)).await
    }

    /// Set the value of GPIO lines
    ///
    /// The value can only be set if the lines have previously been requested as outputs
    /// using the [Chip::request_lines] with [Options::output].
    pub async fn set_values<T: AsValues + Send + 'static>(&self, values: T) -> Result<()> {
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();
        asyncify(move || info.set_values(fd, values)).await
    }
}

/// A Linux chardev GPIO chip interface
///
/// It can be used to get information about the chip and lines and
/// to request GPIO lines that can be used as inputs or outputs.
pub struct Chip {
    info: Arc<Internal<ChipInfo>>,
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

const O_NONBLOCK: i32 = 2048;

impl Chip {
    /// Create a new GPIO chip interface using path
    pub async fn new(path: impl AsRef<Path>) -> Result<Chip> {
        let path = path.as_ref();

        #[allow(unused_assignments)]
        let mut full_path = None;

        let path = if path.starts_with("/dev") {
            path
        } else {
            full_path = Path::new("/dev").join(path).into();
            full_path.as_ref().unwrap()
        };

        let file = File::from_file(
            OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(O_NONBLOCK)
                .open(path)
                .await?,
        )?;

        Chip::check_device(path).await?;

        let fd = file.as_raw_fd();
        let info = Arc::new(asyncify(move || Internal::<ChipInfo>::from_fd(fd)).await?);

        Ok(Chip { info, file })
    }

    /// List all found chips
    pub async fn list_devices() -> Result<Vec<PathBuf>> {
        let mut devices = Vec::new();
        let mut dir = fs::read_dir("/dev").await?;

        while let Some(ent) = dir.next().await {
            let path = ent?.path();
            if Self::check_device(&path).await.is_ok() {
                devices.push(path);
            }
        }

        Ok(devices)
    }

    async fn check_device(path: &Path) -> Result<()> {
        let metadata = fs::symlink_metadata(&path).await?;

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
        ))
        .await?
            != Path::new("/sys/bus/gpio")
        {
            return Err(invalid_input("Character device is not a GPIO"));
        }

        Ok(())
    }

    /// Request the info of a specific GPIO line.
    pub async fn line_info(&self, line: LineId) -> Result<LineInfo> {
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();
        asyncify(move || info.line_info(fd, line)).await
    }

    /// Request the GPIO chip to configure the lines passed as argument as inputs or outputs
    ///
    /// Calling this operation is a precondition to being able to set the state of the GPIO lines.
    /// All the lines passed in one request must share the configured options such as active state, edge detect, GPIO bias, output drive and consumer string.
    /// The state of lines configured as outputs can also be read using the [Outputs::get_values] method.
    pub async fn request_lines<Direction: DirectionType>(
        &self,
        options: Options<Direction, impl AsRef<[LineId]>, impl AsRef<str>>,
    ) -> Result<Direction::Lines> {
        let fd = self.file.as_raw_fd();
        let options = options.to_owned();
        let info = self.info.clone();

        let (info, fd) = asyncify(move || info.request_lines(fd, options)).await?;

        let file = File::from_fd(fd)?;
        let info = Arc::new(info);

        Ok(Direction::lines(info, file))
    }
}
