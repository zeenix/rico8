//! A headless backend: presents nowhere, replays scripted input. Used by `--smoke` and tests.

use crate::platform::{InputSnapshot, Platform};
use anyhow::Result;
use rico8_runtime::fb::Framebuffer;

pub struct NullPlatform {
    scripted: std::collections::VecDeque<InputSnapshot>,
    presented: u32,
}

impl NullPlatform {
    pub fn new() -> NullPlatform {
        NullPlatform {
            scripted: Default::default(),
            presented: 0,
        }
    }

    pub fn scripted(frames: Vec<InputSnapshot>) -> NullPlatform {
        NullPlatform {
            scripted: frames.into(),
            presented: 0,
        }
    }

    pub fn frames_presented(&self) -> u32 {
        self.presented
    }
}

impl Platform for NullPlatform {
    fn present(&mut self, _fb: &Framebuffer) -> Result<()> {
        self.presented += 1;
        Ok(())
    }

    fn poll(&mut self) -> InputSnapshot {
        self.scripted.pop_front().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_then_default() {
        let mut snap = InputSnapshot::default();
        snap.buttons[4] = true;
        let mut p = NullPlatform::scripted(vec![snap]);
        assert!(p.poll().buttons[4], "first poll replays the script");
        assert!(!p.poll().buttons[4], "exhausted script yields default");
        p.present(&Framebuffer::new()).unwrap();
        assert_eq!(p.frames_presented(), 1);
    }
}
