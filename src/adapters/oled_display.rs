use anyhow::{Context, Result};
use embedded_graphics::{
    draw_target::DrawTarget,
    image::{Image, ImageRaw},
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use esp_idf_hal::{
    delay::BLOCK,
    i2c::{I2cConfig, I2cDriver, I2C0},
    units::Hertz,
};
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};

use crate::{platform::pins::DisplayPins, ports::display::DisplayPort};

const DISPLAY_WIDTH: i32 = 128;
const DISPLAY_HEIGHT: i32 = 64;

/// Lopaka export: 41x4 1-bit mouth bitmap.
const IMAGE_PAINT_3_BITS: &[u8] = &[
    0x07, 0x00, 0x00, 0x00, 0xc0, 0x01, 0xfc, 0x01, 0x00, 0x00, 0x7c, 0x00, 0x00, 0x7f, 0x00, 0xe0,
    0x03, 0x00, 0x00, 0x80, 0xff, 0x1f, 0x00, 0x00,
];

pub struct OledDisplayAdapter {
    bus: I2cDriver<'static>,
    address: u8,
    last_status: String,
}

impl OledDisplayAdapter {
    pub fn new(
        i2c: I2C0<'static>,
        pins: DisplayPins,
        address: u8,
        baudrate_hz: u32,
    ) -> Result<Self> {
        let config = I2cConfig::new().baudrate(Hertz(baudrate_hz));
        let bus = I2cDriver::new(i2c, pins.sda, pins.scl, &config)
            .context("failed to create I2C driver for OLED bus")?;

        Ok(Self {
            bus,
            address,
            last_status: String::new(),
        })
    }

    fn cache_status(&mut self, title: &str, detail: &str) {
        self.last_status = format!("{title}: {detail} @0x{:02X}", self.address);
    }

    fn with_buffered_display<F>(&mut self, render: F) -> Result<()>
    where
        F: FnOnce(
            &mut Ssd1306<
                I2CInterface<&mut I2cDriver<'static>>,
                DisplaySize128x64,
                BufferedGraphicsMode<DisplaySize128x64>,
            >,
        ) -> Result<()>,
    {
        let interface = I2CDisplayInterface::new(&mut self.bus);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        display.init().map_err(|error| {
            anyhow::anyhow!("failed to initialize SSD1306 controller: {error:?}")
        })?;
        render(&mut display)?;
        display
            .flush()
            .map_err(|error| anyhow::anyhow!("failed to flush SSD1306 framebuffer: {error:?}"))?;

        Ok(())
    }

    pub fn probe_presence(&mut self) -> Result<bool> {
        match self.bus.write(self.address, &[], BLOCK) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

impl DisplayPort for OledDisplayAdapter {
    fn show_status(&mut self, title: &str, detail: &str) -> Result<()> {
        self.cache_status(title, detail);
        self.with_buffered_display(|display| draw_status_screen(display, title, detail))
    }

    fn run_test_frame(&mut self) -> Result<()> {
        self.cache_status("DISPLAY TEST", "smiling face rendered");
        self.with_buffered_display(|display| draw_face(display))
    }
}

fn draw_status_screen<D>(display: &mut D, title: &str, detail: &str) -> Result<()>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BinaryColor::Off)
        .map_err(|error| anyhow::anyhow!("failed to clear OLED display: {error:?}"))?;

    let title_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();
    let rule_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

    Text::with_baseline(title, Point::new(0, 0), title_style, Baseline::Top)
        .draw(display)
        .map_err(|error| anyhow::anyhow!("failed to draw OLED title text: {error:?}"))?;

    Line::new(Point::new(0, 13), Point::new(DISPLAY_WIDTH - 1, 13))
        .into_styled(rule_style)
        .draw(display)
        .map_err(|error| anyhow::anyhow!("failed to draw OLED separator line: {error:?}"))?;

    for (index, line) in wrap_text(detail, 21).iter().take(4).enumerate() {
        let y = 18 + (index as i32 * 11);
        Text::with_baseline(line, Point::new(0, y), title_style, Baseline::Top)
            .draw(display)
            .map_err(|error| anyhow::anyhow!("failed to draw OLED detail text: {error:?}"))?;
    }

    Ok(())
}

fn draw_face<D>(display: &mut D) -> Result<()>
where
    D: DrawTarget<Color = BinaryColor>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BinaryColor::Off)
        .map_err(|error| anyhow::anyhow!("failed to clear OLED display: {error:?}"))?;

    let fill_style = PrimitiveStyle::with_fill(BinaryColor::On);
    let frame_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

    Rectangle::new(
        Point::new(0, 0),
        Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32),
    )
    .into_styled(frame_style)
    .draw(display)
    .map_err(|error| anyhow::anyhow!("failed to draw OLED face frame: {error:?}"))?;

    Circle::new(Point::new(30, 14), 20)
        .into_styled(fill_style)
        .draw(display)
        .map_err(|error| anyhow::anyhow!("failed to draw OLED left eye: {error:?}"))?;

    Circle::new(Point::new(77, 14), 20)
        .into_styled(fill_style)
        .draw(display)
        .map_err(|error| anyhow::anyhow!("failed to draw OLED right eye: {error:?}"))?;

    Rectangle::new(Point::new(30, 7), Size::new(21, 5))
        .into_styled(fill_style)
        .draw(display)
        .map_err(|error| anyhow::anyhow!("failed to draw OLED left eyebrow: {error:?}"))?;

    Rectangle::new(Point::new(76, 7), Size::new(21, 5))
        .into_styled(fill_style)
        .draw(display)
        .map_err(|error| anyhow::anyhow!("failed to draw OLED right eyebrow: {error:?}"))?;

    let mouth_raw = ImageRaw::<BinaryColor>::new(IMAGE_PAINT_3_BITS, 41);
    Image::new(&mouth_raw, Point::new(40, 50))
        .draw(display)
        .map_err(|error| anyhow::anyhow!("failed to draw OLED mouth bitmap: {error:?}"))?;

    Ok(())
}

fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let extra = if current.is_empty() { 0 } else { 1 };
        if current.len() + word.len() + extra > max_chars && !current.is_empty() {
            lines.push(current);
            current = String::new();
        }

        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
