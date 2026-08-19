#[derive(Default)]
pub struct InputMethod {
    current_mode: u32,
}

impl InputMethod {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_mode(&self) -> u32 {
        self.current_mode
    }

    pub fn set_current_mode(&mut self, mode: u32) {
        self.current_mode = mode;
    }
}
