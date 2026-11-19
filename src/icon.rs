use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSBezierPath, NSColor, NSImage, NSLineCapStyle};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::time::{Duration, Instant};

pub type Levels = [Option<f64>; 2];

const ANIMATION_DURATION: Duration = Duration::from_millis(480);
pub const FRAME_INTERVAL: Duration = Duration::from_millis(32);

#[derive(Clone, Copy)]
pub struct Animation {
    pub generation: u64,
    pub target: Levels,
    from: Levels,
    started: Instant,
}

impl Animation {
    pub fn new(generation: u64, current: Levels, target: Levels) -> Self {
        let from = std::array::from_fn(|i| {
            target[i].map(|next| match current[i] {
                Some(previous) if previous == next => previous * 0.8,
                Some(previous) => previous,
                None => 0.0,
            })
        });
        Self {
            generation,
            target,
            from,
            started: Instant::now(),
        }
    }

    pub fn frame(&self, now: Instant) -> (Levels, bool) {
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed >= ANIMATION_DURATION {
            return (self.target, true);
        }
        let linear = elapsed.as_secs_f64() / ANIMATION_DURATION.as_secs_f64();
        let eased = 1.0 - (1.0 - linear).powi(3);
        let levels = std::array::from_fn(|i| {
            self.target[i].map(|target| {
                let from = self.from[i].unwrap_or(0.0);
                from + (target - from) * eased
            })
        });
        (levels, false)
    }
}

pub fn image(levels: Levels) -> Retained<NSImage> {
    let draw = RcBlock::new(move |_rect: NSRect| -> objc2::runtime::Bool {
        draw_gauge(7.0, levels[0], &[0.0]);
        draw_gauge(22.0, levels[1], &[-1.55, 1.55]);
        objc2::runtime::Bool::YES
    });
    let image =
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(29.0, 16.0), false, &draw);
    image.setTemplate(true);
    image.setAccessibilityDescription(Some(&NSString::from_str(
        "Codex five-hour and weekly usage",
    )));
    image
}

fn draw_gauge(x: f64, remaining: Option<f64>, marks: &[f64]) {
    const Y: f64 = 8.0;
    const RADIUS: f64 = 5.25;
    let track = oval(x, Y, RADIUS);
    track.setLineWidth(1.6);
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
        arc.setLineWidth(1.6);
        arc.setLineCapStyle(NSLineCapStyle::Round);
        arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
            NSPoint::new(x, Y),
            RADIUS,
            90.0,
            90.0 - remaining * 360.0,
            true,
        );
        arc.stroke();
    }
    NSColor::colorWithWhite_alpha(0.0, 0.74).setFill();
    for offset in marks {
        oval(x + offset, Y, 0.83).fill();
    }
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
    fn refresh_animates_known_levels_and_keeps_unknown_levels_unknown() {
        let animation = Animation::new(1, [Some(0.75), Some(0.4)], [Some(0.75), None]);
        let (levels, done) = animation.frame(animation.started);
        assert!(!done);
        assert!(levels[0].is_some_and(|value| (0.59..0.75).contains(&value)));
        assert_eq!(levels[1], None);

        assert_eq!(
            animation.frame(animation.started + ANIMATION_DURATION),
            ([Some(0.75), None], true),
        );
    }
}
