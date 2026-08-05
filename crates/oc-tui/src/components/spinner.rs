//! Spinner frames.
//! From reference/packages/tui/src/ui/spinner.ts (`createFrames`, style "blocks")

/// Block-style spinner frames (8 states, like the reference).
pub fn frames() -> &'static [&'static str] {
    &[
        "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃", "▂",
    ]
}

/// Frame for a given animation tick.
pub fn frame(tick: u64) -> &'static str {
    let f = frames();
    f[(tick as usize) % f.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_cycle() {
        assert_eq!(frame(0), "▁");
        assert_eq!(frame(frames().len() as u64), frame(0));
        assert!(!frames().is_empty());
    }
}
