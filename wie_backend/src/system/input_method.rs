#[derive(Default)]
pub struct InputMethod {
    current_mode: u32,
    eng_key: Option<i8>,
    eng_index: usize,
    eng_char: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputMethodOutput {
    pub handled: bool,
    pub output0: [u8; 8],
    pub output0_len: usize,
    pub output1: [u8; 8],
    pub output1_len: usize,
}

impl Default for InputMethodOutput {
    fn default() -> Self {
        Self {
            handled: false,
            output0: [0; 8],
            output0_len: 0,
            output1: [0; 8],
            output1_len: 0,
        }
    }
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
        self.eng_key = None;
        self.eng_index = 0;
        self.eng_char = None;
    }

    pub fn handle_input(&mut self, key: i8, event: u32) -> InputMethodOutput {
        if !matches!(event, 2 | 4) {
            return InputMethodOutput::default();
        }

        match self.current_mode {
            0 | 1 => self.handle_english(key),
            2 => Self::handle_numeric(key),
            _ => InputMethodOutput::default(),
        }
    }

    fn handle_english(&mut self, key: i8) -> InputMethodOutput {
        if key == -99 {
            let Some(current) = self.eng_char.take() else {
                return InputMethodOutput::default();
            };

            self.eng_key = None;
            self.eng_index = 0;

            let mut output = InputMethodOutput::default();
            output.output0[0] = current;
            output.output0_len = 1;
            return output;
        }

        let upper = self.current_mode == 1;
        let chars: &[u8] = match key {
            48 => b".,?!",
            49 => b"@:/",
            50 => if upper { b"ABC" } else { b"abc" },
            51 => if upper { b"DEF" } else { b"def" },
            52 => if upper { b"GHI" } else { b"ghi" },
            53 => if upper { b"JKL" } else { b"jkl" },
            54 => if upper { b"MNO" } else { b"mno" },
            55 => if upper { b"PQRS" } else { b"pqrs" },
            56 => if upper { b"TUV" } else { b"tuv" },
            57 => if upper { b"WXYZ" } else { b"wxyz" },
            42 => b"*",
            35 => b"#",
            _ => return InputMethodOutput::default(),
        };

        let mut output = InputMethodOutput {
            handled: true,
            ..InputMethodOutput::default()
        };

        if self.eng_key == Some(key) {
            self.eng_index = (self.eng_index + 1) % chars.len();
        } else {
            if let Some(previous) = self.eng_char {
                output.output0[0] = previous;
                output.output0_len = 1;
            }

            self.eng_key = Some(key);
            self.eng_index = 0;
        }

        let current = chars[self.eng_index];
        self.eng_char = Some(current);
        output.output1[0] = current;
        output.output1_len = 1;
        output
    }

    fn handle_numeric(key: i8) -> InputMethodOutput {
        let byte = match key {
            48..=57 | 42 | 35 => key as u8,
            _ => return InputMethodOutput::default(),
        };

        let mut output = InputMethodOutput {
            handled: true,
            ..InputMethodOutput::default()
        };
        output.output0[0] = byte;
        output.output0_len = 1;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::InputMethod;

    #[test]
    fn numeric_mode_matches_native_key_filtering() {
        let mut input = InputMethod::new();
        input.set_current_mode(2);

        for key in [b'0', b'1', b'9', b'*', b'#'] {
            let output = input.handle_input(key as i8, 2);
            assert!(output.handled);
            assert_eq!(output.output0_len, 1);
            assert_eq!(output.output0[0], key);
            assert_eq!(output.output1_len, 0);
        }

        assert!(!input.handle_input(-99, 2).handled);
        assert!(!input.handle_input(b'A' as i8, 2).handled);
        assert!(!input.handle_input(b'1' as i8, 3).handled);
    }
}

#[cfg(test)]
mod english_tests {
    use super::InputMethod;

    #[test]
    fn english_modes_match_native_multitap_state() {
        let mut input = InputMethod::new();

        input.set_current_mode(0);

        let a = input.handle_input(b'2' as i8, 2);
        assert!(a.handled);
        assert_eq!(&a.output1[..a.output1_len], b"a");
        assert_eq!(a.output0_len, 0);

        let b = input.handle_input(b'2' as i8, 2);
        assert!(b.handled);
        assert_eq!(&b.output1[..b.output1_len], b"b");
        assert_eq!(b.output0_len, 0);

        let c = input.handle_input(b'2' as i8, 2);
        assert_eq!(&c.output1[..c.output1_len], b"c");

        let a_again = input.handle_input(b'2' as i8, 2);
        assert_eq!(&a_again.output1[..a_again.output1_len], b"a");

        let d = input.handle_input(b'3' as i8, 2);
        assert!(d.handled);
        assert_eq!(&d.output0[..d.output0_len], b"a");
        assert_eq!(&d.output1[..d.output1_len], b"d");

        let flush = input.handle_input(-99, 2);
        assert!(!flush.handled);
        assert_eq!(&flush.output0[..flush.output0_len], b"d");
        assert_eq!(flush.output1_len, 0);

        input.set_current_mode(1);
        let upper = input.handle_input(b'7' as i8, 2);
        assert!(upper.handled);
        assert_eq!(&upper.output1[..upper.output1_len], b"P");
    }
}
