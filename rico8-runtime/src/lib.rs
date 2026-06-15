//! # rico8-runtime
//!
//! The engine room of RICO-8, the fantasy console: the 128x128 indexed
//! framebuffer and software rasterizer, the built-in pixel font and fixed
//! 16-color palette, the wasmi-based cart sandbox and host ABI, the
//! 4-channel chip-tune synthesizer, the shared asset data models, project
//! storage, and the PNG cartridge format.
//!
//! This crate is windowing- and GPU-agnostic: it renders into a byte
//! buffer and is fully testable headless. The `rico8` console binary puts it on screen
//! with winit + wgpu; carts talk to it through the `rico8` SDK crate.

pub mod assets;
pub mod audio;
pub mod cart;
pub mod fb;
pub mod font;
pub mod input;
pub mod palette;
pub mod pico8;
pub mod project;
pub mod ui;
pub mod vm;
