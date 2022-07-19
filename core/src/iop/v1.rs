use crate::{
    raw::v1::*, utils::*, Active, Bias, BitId, Direction, Drive, Edge, EdgeDetect, Event, LineId,
    LineInfo, Result, Values,
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
        label: &str,
    ) -> Result<Self> {
        let mut request = GpioHandleRequest::default();

        check_len(lines, &request.line_offsets)?;

        request.lines = lines.len() as _;

        request.line_offsets[..lines.len()].copy_from_slice(lines);

        request.flags |= match direction {
            Direction::Input => GPIOHANDLE_REQUEST_INPUT,
            Direction::Output => GPIOHANDLE_REQUEST_INPUT | GPIOHANDLE_REQUEST_OUTPUT,
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

        if let Some(drive) = drive {
            match drive {
                Drive::OpenDrain => request.flags |= GPIOHANDLE_REQUEST_OPEN_DRAIN,
                Drive::OpenSource => request.flags |= GPIOHANDLE_REQUEST_OPEN_SOURCE,
                _ => (),
            }
        }

        safe_set_str(&mut request.consumer_label, label)?;

        Ok(request)
    }
}

impl GpioHandleData {
    pub fn as_values(&self, len: usize) -> Values {
        let mut values = Values::default();
        for i in 0..len {
            values.set(i as _, self.values[i] != 0);
        }
        values
    }

    pub fn from_values(len: usize, values: &Values) -> Self {
        let mut data = GpioHandleData::default();

        for i in 0..len {
            data.values[i] = values.get(i as _).unwrap_or(false) as _;
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
