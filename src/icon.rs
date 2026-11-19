use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSBezierPath, NSColor, NSImage, NSLineCapStyle};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::time::{Duration, Instant};

const ANIMATION_DURATION: Duration = Duration::from_millis(480);
pub const FRAME_INTERVAL: Duration = Duration::from_millis(32);

#[derive(Clone, Copy)]
pub struct Animation {
    pub generation: u64,
    pub target: f64,
    from: f64,
    started: Instant,
}

impl Animation {
    pub fn new(generation: u64, current: Option<f64>, target: f64) -> Self {
        let from = match current {
            Some(previous) if previous == target => previous * 0.8,
            Some(previous) => previous,
            None => 0.0,
        };
        Self {
            generation,
            target,
            from,
            started: Instant::now(),
        }
    }

    pub fn frame(&self, now: Instant) -> (f64, bool) {
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed >= ANIMATION_DURATION {
            return (self.target, true);
        }
        let linear = elapsed.as_secs_f64() / ANIMATION_DURATION.as_secs_f64();
        let eased = 1.0 - (1.0 - linear).powi(3);
        (self.from + (self.target - self.from) * eased, false)
    }
}

pub fn image(remaining: Option<f64>) -> Retained<NSImage> {
    let draw = RcBlock::new(move |_rect: NSRect| -> objc2::runtime::Bool {
        draw_gauge(remaining);
        objc2::runtime::Bool::YES
    });
    let image =
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(20.0, 20.0), false, &draw);
    image.setTemplate(true);
    image.setAccessibilityDescription(Some(&NSString::from_str("Codex five-hour usage")));
    image
}

fn draw_gauge(remaining: Option<f64>) {
    const X: f64 = 10.0;
    const Y: f64 = 10.0;
    const RADIUS: f64 = 8.0;
    const LINE_WIDTH: f64 = 1.8;
    let track = oval(X, Y, RADIUS);
    track.setLineWidth(LINE_WIDTH);
    NSColor::colorWithWhite_alpha(0.0, 0.32).setStroke();
    track.stroke();

    let Some(remaining) = remaining else {
        return;
    };
    NSColor::blackColor().setStroke();
    if remaining >= 1.0 {
        track.stroke();
    } else if remaining > 0.0 {
        let arc = NSBezierPath::bezierPath();
        arc.setLineWidth(LINE_WIDTH);
        arc.setLineCapStyle(NSLineCapStyle::Round);
        arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
            NSPoint::new(X, Y),
            RADIUS,
            90.0,
            90.0 - remaining * 360.0,
            true,
        );
        arc.stroke();
    }
    NSColor::colorWithWhite_alpha(0.0, 0.74).setFill();
    oval(X, Y, 1.1).fill();
}

fn oval(x: f64, y: f64, radius: f64) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(x - radius, y - radius),
        NSSize::new(radius * 2.0, radius * 2.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_animates_even_when_usage_is_unchanged() {
        let animation = Animation::new(1, Some(0.75), 0.75);
        let (level, done) = animation.frame(animation.started);
        assert!(!done);
        assert!((0.59..0.75).contains(&level));

        assert_eq!(
            animation.frame(animation.started + ANIMATION_DURATION),
            (0.75, true),
        );
    }
}
