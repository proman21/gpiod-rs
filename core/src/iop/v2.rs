use crate::{
    raw::v2::*, utils::*, Active, Bias, Direction, Drive, Edge, EdgeDetect, Event, LineId,
    LineInfo, LineMap, Result, Values,
};

/// Raw event ro read from fd
pub type RawEvent = GpioLineEvent;

impl GpioLineInfo {
    pub fn as_info(&self) -> Result<LineInfo> {
        let direction = if is_set(self.flags, GPIO_LINE_FLAG_OUTPUT) {
            Direction::Output
        } else {
            Direction::Input
        };

        let active = if is_set(self.flags, GPIO_LINE_FLAG_ACTIVE_LOW) {
            Active::Low
        } else {
            Active::High
        };

        let edge = match (
            is_set(self.flags, GPIO_LINE_FLAG_EDGE_RISING),
            is_set(self.flags, GPIO_LINE_FLAG_EDGE_FALLING),
        ) {
            (true, false) => EdgeDetect::Rising,
            (false, true) => EdgeDetect::Falling,
            (true, true) => EdgeDetect::Both,
            _ => EdgeDetect::Disable,
        };

        let used = is_set(self.flags, GPIO_LINE_FLAG_USED);

        let bias = match (
            is_set(self.flags, GPIO_LINE_FLAG_BIAS_PULL_UP),
            is_set(self.flags, GPIO_LINE_FLAG_BIAS_PULL_DOWN),
        ) {
            (true, false) => Bias::PullUp,
            (false, true) => Bias::PullDown,
            _ => Bias::Disable,
        };

        let drive = match (
            is_set(self.flags, GPIO_LINE_FLAG_OPEN_DRAIN),
            is_set(self.flags, GPIO_LINE_FLAG_OPEN_SOURCE),
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

impl AsMut<GpioLineValues> for Values {
    fn as_mut(&mut self) -> &mut GpioLineValues {
        // it's safe because data layout is same
        unsafe { &mut *(self as *mut _ as *mut _) }
    }
}

impl GpioLineRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        lines: &[LineId],
        direction: Direction,
        active: Active,
        edge: Option<EdgeDetect>,
        bias: Option<Bias>,
        drive: Option<Drive>,
        values: Option<Values>,
        label: &str,
    ) -> Result<Self> {
        let mut request = GpioLineRequest::default();

        check_len(lines, &request.offsets)?;

        request.num_lines = lines.len() as _;

        request.offsets.copy_from_slice(lines);

        let config = &mut request.config;

        config.flags |= match direction {
            Direction::Input => GPIO_LINE_FLAG_INPUT,
            Direction::Output => GPIO_LINE_FLAG_INPUT | GPIO_LINE_FLAG_OUTPUT,
        };

        if matches!(active, Active::Low) {
            config.flags |= GPIO_LINE_FLAG_ACTIVE_LOW;
        }

        if let Some(edge) = edge {
            match edge {
                EdgeDetect::Rising => config.flags |= GPIO_LINE_FLAG_EDGE_RISING,
                EdgeDetect::Falling => config.flags |= GPIO_LINE_FLAG_EDGE_FALLING,
                EdgeDetect::Both => config.flags |= GPIO_LINE_FLAG_EDGE_BOTH,
                _ => {}
            }
        }

        if let Some(bias) = bias {
            config.flags |= match bias {
                Bias::PullUp => GPIO_LINE_FLAG_BIAS_PULL_UP,
                Bias::PullDown => GPIO_LINE_FLAG_BIAS_PULL_DOWN,
                Bias::Disable => GPIO_LINE_FLAG_BIAS_DISABLED,
            }
        }

        if let Some(drive) = drive {
            match drive {
                Drive::OpenDrain => config.flags |= GPIO_LINE_FLAG_OPEN_DRAIN,
                Drive::OpenSource => config.flags |= GPIO_LINE_FLAG_OPEN_SOURCE,
                _ => (),
            }
        }

        if matches!(direction, Direction::Output) {
            if let Some(values) = values {
                config.num_attrs = 1;
                let attr = &mut config.attrs[0];
                attr.attr.id = GPIO_LINE_ATTR_ID_OUTPUT_VALUES;
                attr.mask = values.mask;
                attr.attr.val.values = values.bits;
            }
        }

        safe_set_str(&mut request.consumer, label)?;

        Ok(request)
    }
}

impl GpioLineEvent {
    pub fn as_event(&self, line_map: &LineMap) -> Result<Event> {
        let line = line_map.get(self.offset)?;

        let edge = match self.id {
            GPIO_LINE_EVENT_RISING_EDGE => Edge::Rising,
            GPIO_LINE_EVENT_FALLING_EDGE => Edge::Falling,
            _ => return Err(invalid_data("Unknown edge")),
        };

        let time = time_from_nanos(self.timestamp_ns);

        Ok(Event { line, edge, time })
    }
}
