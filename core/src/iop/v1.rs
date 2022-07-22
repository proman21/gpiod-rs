use crate::{
    raw::v1::*, utils::*, Active, AsValues, AsValuesMut, Bias, BitId, Direction, Drive, Edge,
    EdgeDetect, Event, LineId, LineInfo, Result,
};

/// Raw event to read from fd
pub type RawEvent = GpioEventData;

impl GpioLineInfo {
    pub fn as_info(&self) -> Result<LineInfo> {
        let direction = if is_set(self.flags, GPIOLINE_FLAG_IS_OUT) {
            Direction::Output
        } else {
            Direction::Input
        };

        let active = if is_set(self.flags, GPIOLINE_FLAG_ACTIVE_LOW) {
            Active::Low
        } else {
            Active::High
        };

        let edge = EdgeDetect::Disable;

        let used = is_set(self.flags, GPIOLINE_FLAG_KERNEL);

        let bias = match (
            is_set(self.flags, GPIOLINE_FLAG_BIAS_PULL_UP),
            is_set(self.flags, GPIOLINE_FLAG_BIAS_PULL_DOWN),
        ) {
            (true, false) => Bias::PullUp,
            (false, true) => Bias::PullDown,
            _ => Bias::Disable,
        };

        let drive = match (
            is_set(self.flags, GPIOLINE_FLAG_OPEN_DRAIN),
            is_set(self.flags, GPIOLINE_FLAG_OPEN_SOURCE),
        ) {
            (true, false) => Drive::OpenDrain,
            (false, true) => Drive::OpenSource,
            _ => Drive::PushPull,
        };
        let name = safe_get_str(&self.name)?.into();
        let consumer = safe_get_str(&self.consumer)?.into();

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

impl GpioHandleRequest {
    pub fn new(
        lines: &[LineId],
        direction: Direction,
        active: Active,
        bias: Option<Bias>,
        drive: Option<Drive>,
        consumer: &str,
    ) -> Result<Self> {
        let mut request = GpioHandleRequest::default();

        check_len(lines, &request.line_offsets)?;

        request.lines = lines.len() as _;

        request.line_offsets[..lines.len()].copy_from_slice(lines);

        request.flags |= match direction {
            Direction::Input => GPIOHANDLE_REQUEST_INPUT,
            // Mixing input and output flags is not allowed
            // see https://github.com/torvalds/linux/blob/v5.18/drivers/gpio/gpiolib-cdev.c#L92-L98
            Direction::Output => GPIOHANDLE_REQUEST_OUTPUT,
        };

        if matches!(active, Active::Low) {
            request.flags |= GPIOHANDLE_REQUEST_ACTIVE_LOW;
        }

        if let Some(bias) = bias {
            request.flags |= match bias {
                Bias::PullUp => GPIOHANDLE_REQUEST_BIAS_PULL_UP,
                Bias::PullDown => GPIOHANDLE_REQUEST_BIAS_PULL_DOWN,
                Bias::Disable => GPIOHANDLE_REQUEST_BIAS_DISABLE,
            };
        }

        if matches!(direction, Direction::Output) {
            // Set drive flags is valid only for output
            // see https://github.com/torvalds/linux/blob/v5.18/drivers/gpio/gpiolib-cdev.c#L109-L113
            if let Some(drive) = drive {
                match drive {
                    Drive::OpenDrain => request.flags |= GPIOHANDLE_REQUEST_OPEN_DRAIN,
                    Drive::OpenSource => request.flags |= GPIOHANDLE_REQUEST_OPEN_SOURCE,
                    _ => (),
                }
            }
        }

        safe_set_str(&mut request.consumer_label, consumer)?;

        Ok(request)
    }
}

impl GpioHandleData {
    pub fn fill_values(&self, len: usize, values: &mut impl AsValuesMut) {
        for id in 0..len {
            values.set(id as _, Some(self.values[id] != 0));
        }
    }

    pub fn from_values(len: usize, values: impl AsValues) -> Self {
        let mut data = GpioHandleData::default();

        for i in 0..len {
            data.values[i] = if values.get(i as _).unwrap_or(false) {
                1
            } else {
                0
            };
        }

        data
    }
}

impl GpioEventData {
    pub fn as_event(&self, line: BitId) -> Result<Event> {
        let edge = match self.id {
            GPIOEVENT_EVENT_RISING_EDGE => Edge::Rising,
            GPIOEVENT_EVENT_FALLING_EDGE => Edge::Falling,
            _ => return Err(invalid_data("Unknown edge")),
        };

        let time = time_from_nanos(self.timestamp);

        Ok(Event { line, edge, time })
    }
}
