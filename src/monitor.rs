pub struct Monitor {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Monitor {
    pub fn width_from_percentage(&self, p: f32) -> u16 {
        (self.width as f32 * p).floor() as u16
    }

    pub fn client_height(&self, client_count: usize) -> u16 {
        self.height / client_count as u16
    }
}
