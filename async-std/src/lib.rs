#![doc = include_str!("../README.md")]

use std::{
    fmt, io,
    ops::Deref,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::{AsRawFd, FromRawFd, RawFd},
    },
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use gpiod_core::{invalid_input, major, minor, Error, Internal, Result};

pub use gpiod_core::{
    Active, Bias, BitId, ChipInfo, Direction, Drive, Edge, EdgeDetect, Event, LineId, LineInfo,
    Values, ValuesInfo, ValuesIter,
};

use async_io::Async;
use async_std::{
    fs,
    fs::OpenOptions,
    io::{Read, ReadExt},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    stream::StreamExt,
    task::spawn_blocking,
};

async fn asyncify<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    match spawn_blocking(f).await {
        Ok(res) => Ok(res),
        Err(_) => Err(Error::new(io::ErrorKind::Other, "background task failed")),
    }
}

struct File {
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

/// The interface for getting the values of GPIO lines configured for input
///
/// Use [Chip::request_input] to configure specific GPIO lines for input.
pub struct Inputs {
    info: Arc<Internal<ValuesInfo>>,
    // wrap file to call close on drop
    file: File,
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
    /// The values can only be read if the lines have previously been requested as either inputs
    /// using the [Chip::request_input] method, or outputs using the [Chip::request_output].
    pub async fn get_values<T: From<Values>>(&self) -> Result<T> {
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();
        Ok(asyncify(move || info.get_values(fd)).await?.into())
    }

    /// Read GPIO events synchronously
    pub async fn read_event(&mut self) -> Result<Event> {
        read_event(self.info.index(), &mut self.file).await
    }
}

/// The interface for setting the values of GPIO lines configured for output
///
/// Use [Chip::request_output] to configure specific GPIO lines for output.
///
/// The values also can be read.
/// Specifically this may be useful to get actual value when lines driven as open drain or source.
pub struct Outputs {
    info: Arc<Internal<ValuesInfo>>,
    // wrap file to call close on drop
    file: File,
}

impl Deref for Outputs {
    type Target = ValuesInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
}

impl Outputs {
    /// Get the value of GPIO lines
    ///
    /// The values can only be read if the lines have previously been requested as either inputs
    /// using the [Chip::request_input] method, or outputs using the [Chip::request_output].
    pub async fn get_values<T: From<Values>>(&self) -> Result<T> {
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();
        Ok(asyncify(move || info.get_values(fd)).await?.into())
    }

    /// Set the value of GPIO lines
    ///
    /// The value can only be set if the lines have previously been requested as outputs
    /// using the [Chip::request_output].
    pub async fn set_values(&self, values: impl Into<Values>) -> Result<()> {
        let values = values.into();
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();
        asyncify(move || info.set_values(fd, values)).await
    }

    /// Read GPIO events synchronously
    pub async fn read_event(&mut self) -> Result<Event> {
        read_event(self.info.index(), &mut self.file).await
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

    /// Request the GPIO chip to configure the lines passed as argument as outputs
    ///
    /// Calling this operation is a precondition to being able to set the state of the GPIO lines.
    /// All the lines passed in one request must share the output mode and the active state.
    /// The state of lines configured as outputs can also be read using the [Outputs::get_values] method.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_output(
        &self,
        lines: impl AsRef<[LineId]>,
        active: Active,
        edge: EdgeDetect,
        bias: Bias,
        drive: Drive,
        values: Option<impl Into<Values>>,
        label: impl AsRef<str>,
    ) -> Result<Outputs> {
        let lines = lines.as_ref().to_owned();
        let label = label.as_ref().to_owned();
        let values = values.map(Into::into);
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();

        let (info, fd) = asyncify(move || {
            info.request_lines(
                fd,
                &lines,
                Direction::Output,
                active,
                Some(edge),
                Some(bias),
                Some(drive),
                values,
                &label,
            )
        })
        .await?;

        let file = File::from_fd(fd)?;
        let info = Arc::new(info);

        Ok(Outputs { info, file })
    }

    /// Request the GPIO chip to configure the lines passed as argument as inputs
    ///
    /// Calling this operation is a precondition to being able to read the state of the GPIO lines.
    pub async fn request_input(
        &self,
        lines: impl AsRef<[LineId]>,
        active: Active,
        edge: EdgeDetect,
        bias: Bias,
        label: impl AsRef<str>,
    ) -> Result<Inputs> {
        let lines = lines.as_ref().to_owned();
        let label = label.as_ref().to_owned();
        let fd = self.file.as_raw_fd();
        let info = self.info.clone();

        let (info, fd) = asyncify(move || {
            info.request_lines(
                fd,
                &lines,
                Direction::Output,
                active,
                Some(edge),
                Some(bias),
                None,
                None,
                &label,
            )
        })
        .await?;

        let file = File::from_fd(fd)?;
        let info = Arc::new(info);

        Ok(Inputs { info, file })
    }
}
