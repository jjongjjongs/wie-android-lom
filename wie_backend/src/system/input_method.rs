use alloc::vec::Vec;

use encoding_rs::EUC_KR;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct KoreanState {
    cho: Option<u8>,
    jung: Option<u8>,
    jong: Option<u8>,
    vowel_state: u8,
    consonant_scan: Option<u8>,
    last_key: Option<i8>,
    vowel_toggle: bool,
}

#[derive(Default)]
pub struct InputMethod {
    current_mode: u32,
    composition_size: usize,
    eng_key: Option<i8>,
    eng_index: usize,
    eng_char: Option<u8>,

    ko_cho: Option<u8>,
    ko_jung: Option<u8>,
    ko_jong: Option<u8>,
    ko_vowel_state: u8,
    ko_consonant_scan: Option<u8>,
    ko_last_key: Option<i8>,
    ko_vowel_toggle: bool,
    ko_undo: Vec<KoreanState>,
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

    pub fn composition_size(&self) -> usize {
        self.composition_size
    }

    pub fn set_composition_size(&mut self, size: usize) {
        self.composition_size = size;
    }

    pub fn set_current_mode(&mut self, mode: u32) {
        self.current_mode = mode;
        self.eng_key = None;
        self.eng_index = 0;
        self.eng_char = None;

        self.ko_cho = None;
        self.ko_jung = None;
        self.ko_jong = None;
        self.ko_vowel_state = 0;
        self.ko_consonant_scan = None;
        self.ko_last_key = None;
        self.ko_vowel_toggle = false;
        self.ko_undo.clear();
    }

    pub fn handle_input(&mut self, key: i8, event: u32) -> InputMethodOutput {
        if !matches!(event, 2 | 4) {
            return InputMethodOutput::default();
        }

        match self.current_mode {
            0 | 1 => self.handle_english(key),
            2 => Self::handle_numeric(key),
            3 => self.handle_korean(key),
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
            50 => {
                if upper {
                    b"ABC"
                } else {
                    b"abc"
                }
            }
            51 => {
                if upper {
                    b"DEF"
                } else {
                    b"def"
                }
            }
            52 => {
                if upper {
                    b"GHI"
                } else {
                    b"ghi"
                }
            }
            53 => {
                if upper {
                    b"JKL"
                } else {
                    b"jkl"
                }
            }
            54 => {
                if upper {
                    b"MNO"
                } else {
                    b"mno"
                }
            }
            55 => {
                if upper {
                    b"PQRS"
                } else {
                    b"pqrs"
                }
            }
            56 => {
                if upper {
                    b"TUV"
                } else {
                    b"tuv"
                }
            }
            57 => {
                if upper {
                    b"WXYZ"
                } else {
                    b"wxyz"
                }
            }
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

    fn korean_cho_index(cho: u8) -> Option<u32> {
        match cho {
            2 => Some(0),   // ㄱ
            3 => Some(1),   // ㄲ
            4 => Some(2),   // ㄴ
            5 => Some(3),   // ㄷ
            6 => Some(4),   // ㄸ
            7 => Some(5),   // ㄹ
            8 => Some(6),   // ㅁ
            9 => Some(7),   // ㅂ
            10 => Some(8),  // ㅃ
            11 => Some(9),  // ㅅ
            12 => Some(10), // ㅆ
            13 => Some(11), // ㅇ
            14 => Some(12), // ㅈ
            15 => Some(13), // ㅉ
            16 => Some(14), // ㅊ
            17 => Some(15), // ㅋ
            18 => Some(16), // ㅌ
            19 => Some(17), // ㅍ
            20 => Some(18), // ㅎ
            _ => None,
        }
    }

    fn korean_jung_index(jung: u8) -> Option<u32> {
        match jung {
            3 => Some(0),   // ㅏ
            4 => Some(1),   // ㅐ
            5 => Some(2),   // ㅑ
            6 => Some(3),   // ㅒ
            7 => Some(4),   // ㅓ
            10 => Some(5),  // ㅔ
            11 => Some(6),  // ㅕ
            12 => Some(7),  // ㅖ
            13 => Some(8),  // ㅗ
            14 => Some(9),  // ㅘ
            15 => Some(10), // ㅙ
            18 => Some(11), // ㅚ
            19 => Some(12), // ㅛ
            20 => Some(13), // ㅜ
            21 => Some(14), // ㅝ
            22 => Some(15), // ㅞ
            23 => Some(16), // ㅟ
            26 => Some(17), // ㅠ
            27 => Some(18), // ㅡ
            28 => Some(19), // ㅢ
            29 => Some(20), // ㅣ
            _ => None,
        }
    }

    fn korean_jong_index(jong: u8) -> Option<u32> {
        match jong {
            1 => Some(0),
            2 => Some(1),
            3 => Some(2),
            4 => Some(3),
            5 => Some(4),
            6 => Some(5),
            7 => Some(6),
            8 => Some(7),
            9 => Some(8),
            10 => Some(9),
            11 => Some(10),
            12 => Some(11),
            13 => Some(12),
            14 => Some(13),
            15 => Some(14),
            16 => Some(15),
            17 => Some(16),
            19 => Some(17),
            20 => Some(18),
            21 => Some(19),
            22 => Some(20),
            23 => Some(21),
            24 => Some(22),
            25 => Some(23),
            26 => Some(24),
            27 => Some(25),
            28 => Some(26),
            29 => Some(27),
            _ => None,
        }
    }

    fn encode_korean_char(ch: char) -> Option<([u8; 2], usize)> {
        let mut utf8 = [0u8; 4];
        let text = ch.encode_utf8(&mut utf8);
        let (encoded, _, had_errors) = EUC_KR.encode(text);
        if had_errors || encoded.is_empty() || encoded.len() > 2 {
            return None;
        }

        let mut bytes = [0u8; 2];
        bytes[..encoded.len()].copy_from_slice(&encoded);
        Some((bytes, encoded.len()))
    }

    fn compose_korean_syllable(cho: u8, jung: u8, jong: u8) -> Option<char> {
        let cho = Self::korean_cho_index(cho)?;
        let jung = Self::korean_jung_index(jung)?;
        let jong = Self::korean_jong_index(jong)?;
        char::from_u32(0xac00 + (cho * 21 + jung) * 28 + jong)
    }

    fn korean_jong_to_cho(jong: u8) -> Option<u8> {
        match jong {
            2 => Some(2),
            3 => Some(3),
            5 => Some(4),
            8 => Some(5),
            9 => Some(7),
            17 => Some(8),
            19 => Some(9),
            21 => Some(11),
            22 => Some(12),
            23 => Some(13),
            24 => Some(14),
            25 => Some(16),
            26 => Some(17),
            27 => Some(18),
            28 => Some(19),
            29 => Some(20),
            _ => None,
        }
    }

    fn combine_korean_jong(first: u8, second: u8) -> Option<u8> {
        match (first, second) {
            (2, 21) => Some(4),   // ㄱ + ㅅ -> ㄳ
            (5, 24) => Some(6),   // ㄴ + ㅈ -> ㄵ
            (5, 29) => Some(7),   // ㄴ + ㅎ -> ㄶ
            (9, 2) => Some(10),   // ㄹ + ㄱ -> ㄺ
            (9, 17) => Some(11),  // ㄹ + ㅁ -> ㄻ
            (9, 19) => Some(12),  // ㄹ + ㅂ -> ㄼ
            (9, 21) => Some(13),  // ㄹ + ㅅ -> ㄽ
            (9, 27) => Some(14),  // ㄹ + ㅌ -> ㄾ
            (9, 28) => Some(15),  // ㄹ + ㅍ -> ㄿ
            (9, 29) => Some(16),  // ㄹ + ㅎ -> ㅀ
            (19, 21) => Some(20), // ㅂ + ㅅ -> ㅄ
            _ => None,
        }
    }

    fn korean_scan_to_jong(scan: u8) -> Option<u8> {
        match scan {
            2 => Some(2),   // ㄱ
            3 => Some(3),   // ㄲ
            4 => Some(5),   // ㄴ
            5 => Some(8),   // ㄷ
            7 => Some(9),   // ㄹ
            8 => Some(17),  // ㅁ
            9 => Some(19),  // ㅂ
            11 => Some(21), // ㅅ
            12 => Some(22), // ㅆ
            13 => Some(23), // ㅇ
            14 => Some(24), // ㅈ
            16 => Some(25), // ㅊ
            17 => Some(26), // ㅋ
            18 => Some(27), // ㅌ
            19 => Some(28), // ㅍ
            20 => Some(29), // ㅎ
            _ => None,
        }
    }

    fn korean_vowel_jung(state: u8) -> Option<u8> {
        match state {
            1 => Some(3),   // ㅏ
            2 => Some(5),   // ㅑ
            3 => Some(6),   // ㅒ
            4 => Some(4),   // ㅐ
            5 => Some(7),   // ㅓ
            6 => Some(11),  // ㅕ
            7 => Some(12),  // ㅖ
            8 => Some(10),  // ㅔ
            9 => Some(13),  // ㅗ
            10 => Some(19), // ㅛ
            11 => Some(18), // ㅚ
            12 => Some(14), // ㅘ
            13 => Some(15), // ㅙ
            14 => Some(20), // ㅜ
            15 => Some(26), // ㅠ
            16 => Some(21), // ㅝ
            17 => Some(22), // ㅞ
            18 => Some(23), // ㅟ
            19 => Some(27), // ㅡ
            20 => Some(28), // ㅢ
            21 => Some(29), // ㅣ
            _ => None,
        }
    }

    fn korean_vowel_transition(state: u8, scan: u8) -> Option<(u8, bool)> {
        match (state, scan) {
            (0, 21 | 22) => Some((1, false)),
            (0, 23 | 24) => Some((9, false)),
            (0, 25) => Some((19, false)),
            (0, 26) => Some((21, false)),

            (1, 21 | 22) => Some((5, true)),
            (1, 26) => Some((4, false)),
            (1, 27 | 28) => Some((2, false)),

            (2, 26) => Some((3, false)),
            (2, 27) => Some((1, false)),

            (5, 21 | 22) => Some((1, true)),
            (5, 26) => Some((8, false)),
            (5, 27 | 28) => Some((6, false)),

            (6, 26) => Some((7, false)),
            (6, 27) => Some((5, false)),

            (9, 21 | 22) => Some((12, false)),
            (9, 23 | 24) => Some((14, true)),
            (9, 26) => Some((11, false)),
            (9, 27 | 28) => Some((10, false)),

            (10, 27) => Some((9, true)),

            (12, 26) => Some((13, false)),

            (14, 21 | 22) => Some((16, false)),
            (14, 23 | 24) => Some((9, true)),
            (14, 26) => Some((18, false)),
            (14, 27 | 28) => Some((15, false)),

            (15, 27) => Some((14, false)),

            (16, 26) => Some((17, false)),

            (19, 26) => Some((20, true)),
            (19, 27 | 28) => Some((20, false)),

            _ => None,
        }
    }

    fn initial_korean_scan(key: i8) -> Option<u8> {
        match key {
            49 => Some(2),  // 1 -> ㄱ
            50 => Some(4),  // 2 -> ㄴ
            51 => Some(21), // 3 -> ㅏ-series
            52 => Some(7),  // 4 -> ㄹ
            53 => Some(8),  // 5 -> ㅁ
            54 => Some(23), // 6 -> ㅗ-series
            55 => Some(11), // 7 -> ㅅ
            56 => Some(13), // 8 -> ㅇ
            57 => Some(25), // 9 -> ㅡ-series
            48 => Some(26), // 0 -> ㅣ
            _ => None,
        }
    }

    fn modify_korean_consonant(scan: u8, key: i8) -> Option<u8> {
        match (scan, key) {
            (2, 42) => Some(17),
            (2, 35) => Some(3),
            (3, 35) => Some(2),
            (17, 42) => Some(2),

            (4, 42) => Some(5),
            (5, 42) => Some(18),
            (5, 35) => Some(6),
            (6, 35) => Some(5),
            (18, 42) => Some(4),

            (8, 42) => Some(9),
            (9, 42) => Some(19),
            (9, 35) => Some(10),
            (10, 35) => Some(9),
            (19, 42) => Some(8),

            (11, 42) => Some(14),
            (11, 35) => Some(12),
            (12, 35) => Some(11),
            (14, 42) => Some(16),
            (14, 35) => Some(15),
            (15, 35) => Some(14),
            (16, 42) => Some(11),

            (13, 42) => Some(20),
            (20, 42) => Some(13),

            _ => None,
        }
    }

    fn korean_cho_char(cho: u8) -> Option<char> {
        match cho {
            2 => Some('ㄱ'),
            3 => Some('ㄲ'),
            4 => Some('ㄴ'),
            5 => Some('ㄷ'),
            6 => Some('ㄸ'),
            7 => Some('ㄹ'),
            8 => Some('ㅁ'),
            9 => Some('ㅂ'),
            10 => Some('ㅃ'),
            11 => Some('ㅅ'),
            12 => Some('ㅆ'),
            13 => Some('ㅇ'),
            14 => Some('ㅈ'),
            15 => Some('ㅉ'),
            16 => Some('ㅊ'),
            17 => Some('ㅋ'),
            18 => Some('ㅌ'),
            19 => Some('ㅍ'),
            20 => Some('ㅎ'),
            _ => None,
        }
    }

    fn korean_jung_char(jung: u8) -> Option<char> {
        match jung {
            3 => Some('ㅏ'),
            4 => Some('ㅐ'),
            5 => Some('ㅑ'),
            6 => Some('ㅒ'),
            7 => Some('ㅓ'),
            10 => Some('ㅔ'),
            11 => Some('ㅕ'),
            12 => Some('ㅖ'),
            13 => Some('ㅗ'),
            14 => Some('ㅘ'),
            15 => Some('ㅙ'),
            18 => Some('ㅚ'),
            19 => Some('ㅛ'),
            20 => Some('ㅜ'),
            21 => Some('ㅝ'),
            22 => Some('ㅞ'),
            23 => Some('ㅟ'),
            26 => Some('ㅠ'),
            27 => Some('ㅡ'),
            28 => Some('ㅢ'),
            29 => Some('ㅣ'),
            _ => None,
        }
    }

    fn split_korean_jong(jong: u8) -> Option<(u8, u8)> {
        match jong {
            4 => Some((2, 21)),
            6 => Some((5, 24)),
            7 => Some((5, 29)),
            10 => Some((9, 2)),
            11 => Some((9, 17)),
            12 => Some((9, 19)),
            13 => Some((9, 21)),
            14 => Some((9, 27)),
            15 => Some((9, 28)),
            16 => Some((9, 29)),
            20 => Some((19, 21)),
            _ => None,
        }
    }

    fn current_korean_char(&self) -> Option<char> {
        match (self.ko_cho, self.ko_jung) {
            (Some(cho), Some(jung)) => Self::compose_korean_syllable(cho, jung, self.ko_jong.unwrap_or(1)),
            (Some(cho), None) => Self::korean_cho_char(cho),
            (None, Some(jung)) => Self::korean_jung_char(jung),
            (None, None) => None,
        }
    }

    fn put_korean_char(output: &mut [u8; 8], output_len: &mut usize, ch: char) -> bool {
        let Some((bytes, len)) = Self::encode_korean_char(ch) else {
            return false;
        };

        output[..len].copy_from_slice(&bytes[..len]);
        *output_len = len;
        true
    }

    fn reset_korean_composition(&mut self) {
        self.ko_cho = None;
        self.ko_jung = None;
        self.ko_jong = None;
        self.ko_vowel_state = 0;
        self.ko_consonant_scan = None;
    }

    fn korean_state(&self) -> KoreanState {
        KoreanState {
            cho: self.ko_cho,
            jung: self.ko_jung,
            jong: self.ko_jong,
            vowel_state: self.ko_vowel_state,
            consonant_scan: self.ko_consonant_scan,
            last_key: self.ko_last_key,
            vowel_toggle: self.ko_vowel_toggle,
        }
    }

    fn restore_korean_state(&mut self, state: KoreanState) {
        self.ko_cho = state.cho;
        self.ko_jung = state.jung;
        self.ko_jong = state.jong;
        self.ko_vowel_state = state.vowel_state;
        self.ko_consonant_scan = state.consonant_scan;
        self.ko_last_key = state.last_key;
        self.ko_vowel_toggle = state.vowel_toggle;
    }

    fn clear_korean_input(&mut self) -> InputMethodOutput {
        let state = self.ko_undo.pop().unwrap_or_default();
        self.restore_korean_state(state);

        let mut output = InputMethodOutput {
            handled: true,
            ..InputMethodOutput::default()
        };

        if let Some(ch) = self.current_korean_char() {
            Self::put_korean_char(&mut output.output1, &mut output.output1_len, ch);
        }

        output
    }

    fn start_korean_vowel(&mut self, scan: u8) -> bool {
        let Some((state, _)) = Self::korean_vowel_transition(0, scan) else {
            return false;
        };
        let Some(jung) = Self::korean_vowel_jung(state) else {
            return false;
        };

        self.ko_vowel_state = state;
        self.ko_jung = Some(jung);
        true
    }

    fn scan_korean_key(&mut self, key: i8) -> Option<(u8, bool)> {
        if matches!(key, 42 | 35) {
            if key == 42 && self.ko_vowel_state != 0 && matches!(self.ko_last_key, Some(48 | 51 | 54 | 57)) {
                self.ko_last_key = Some(key);
                return Some((27, false));
            }

            if let Some(scan) = self.ko_consonant_scan {
                if let Some(modified) = Self::modify_korean_consonant(scan, key) {
                    self.ko_consonant_scan = Some(modified);
                    self.ko_last_key = Some(key);
                    return Some((modified, true));
                }
            }

            self.ko_last_key = Some(key);
            return None;
        }

        let scan = match key {
            51 => {
                if self.ko_last_key == Some(51) {
                    self.ko_vowel_toggle = !self.ko_vowel_toggle;
                    if self.ko_vowel_toggle { 21 } else { 22 }
                } else {
                    self.ko_vowel_toggle = true;
                    21
                }
            }
            54 => {
                if self.ko_last_key == Some(54) {
                    self.ko_vowel_toggle = !self.ko_vowel_toggle;
                    if self.ko_vowel_toggle { 23 } else { 24 }
                } else {
                    self.ko_vowel_toggle = true;
                    23
                }
            }
            57 => {
                if self.ko_last_key == Some(57) {
                    self.ko_vowel_toggle = !self.ko_vowel_toggle;
                } else {
                    self.ko_vowel_toggle = true;
                }
                25
            }
            48 => {
                self.ko_vowel_toggle = false;
                26
            }
            _ => {
                self.ko_vowel_toggle = false;
                Self::initial_korean_scan(key)?
            }
        };

        self.ko_last_key = Some(key);
        Some((scan, false))
    }

    fn handle_korean(&mut self, key: i8) -> InputMethodOutput {
        if key == -99 {
            let mut output = InputMethodOutput::default();
            if let Some(ch) = self.current_korean_char() {
                Self::put_korean_char(&mut output.output0, &mut output.output0_len, ch);
            }
            self.reset_korean_composition();
            self.ko_last_key = None;
            self.ko_vowel_toggle = false;
            self.ko_undo.clear();
            return output;
        }

        if key == -16 {
            return self.clear_korean_input();
        }

        // Capture before scan_korean_key mutates the key/toggle metadata.
        let before = self.korean_state();

        let Some((scan, modifier)) = self.scan_korean_key(key) else {
            return InputMethodOutput::default();
        };

        // A commit normally rebases CLEAR to an empty composition. Jong split
        // is the exception: part of the old jong becomes the new live cho.
        let mut commit_baseline = KoreanState::default();

        let mut output = InputMethodOutput {
            handled: true,
            ..InputMethodOutput::default()
        };

        if Self::korean_cho_index(scan).is_some() {
            if modifier {
                if self.ko_jong.is_some() {
                    if let Some(jong) = Self::korean_scan_to_jong(scan) {
                        self.ko_jong = Some(jong);
                    }
                } else if self.ko_cho.is_some() {
                    self.ko_cho = Some(scan);
                }

                if let Some(ch) = self.current_korean_char() {
                    Self::put_korean_char(&mut output.output1, &mut output.output1_len, ch);
                }
                self.ko_undo.push(before);
                return output;
            }

            self.ko_vowel_state = 0;
            self.ko_consonant_scan = Some(scan);

            match (self.ko_cho, self.ko_jung, self.ko_jong) {
                (None, None, None) => {
                    self.ko_cho = Some(scan);
                }
                (Some(_), None, None) => {
                    if let Some(ch) = self.current_korean_char() {
                        Self::put_korean_char(&mut output.output0, &mut output.output0_len, ch);
                    }
                    self.reset_korean_composition();
                    self.ko_cho = Some(scan);
                    self.ko_consonant_scan = Some(scan);
                }
                (Some(_), Some(_), None) => {
                    if let Some(jong) = Self::korean_scan_to_jong(scan) {
                        self.ko_jong = Some(jong);
                    } else {
                        if let Some(ch) = self.current_korean_char() {
                            Self::put_korean_char(&mut output.output0, &mut output.output0_len, ch);
                        }
                        self.reset_korean_composition();
                        self.ko_cho = Some(scan);
                        self.ko_consonant_scan = Some(scan);
                    }
                }
                (Some(_), Some(_), Some(jong)) => {
                    if let Some(second) = Self::korean_scan_to_jong(scan) {
                        if let Some(combined) = Self::combine_korean_jong(jong, second) {
                            self.ko_jong = Some(combined);
                        } else {
                            if let Some(ch) = self.current_korean_char() {
                                Self::put_korean_char(&mut output.output0, &mut output.output0_len, ch);
                            }
                            self.reset_korean_composition();
                            self.ko_cho = Some(scan);
                            self.ko_consonant_scan = Some(scan);
                        }
                    } else {
                        if let Some(ch) = self.current_korean_char() {
                            Self::put_korean_char(&mut output.output0, &mut output.output0_len, ch);
                        }
                        self.reset_korean_composition();
                        self.ko_cho = Some(scan);
                        self.ko_consonant_scan = Some(scan);
                    }
                }
                _ => {
                    self.reset_korean_composition();
                    self.ko_cho = Some(scan);
                    self.ko_consonant_scan = Some(scan);
                }
            }

            if let Some(ch) = self.current_korean_char() {
                Self::put_korean_char(&mut output.output1, &mut output.output1_len, ch);
            }

            if output.output0_len != 0 {
                self.ko_undo.clear();
                self.ko_undo.push(commit_baseline);
            } else {
                self.ko_undo.push(before);
            }
            return output;
        }

        if !(21..=28).contains(&scan) {
            return InputMethodOutput::default();
        }

        if let Some(jong) = self.ko_jong {
            let Some(old_cho) = self.ko_cho else {
                return InputMethodOutput::default();
            };
            let Some(old_jung) = self.ko_jung else {
                return InputMethodOutput::default();
            };

            if let Some((first, second)) = Self::split_korean_jong(jong) {
                if let Some(committed) = Self::compose_korean_syllable(old_cho, old_jung, first) {
                    Self::put_korean_char(&mut output.output0, &mut output.output0_len, committed);
                }

                self.reset_korean_composition();
                self.ko_cho = Self::korean_jong_to_cho(second);
                self.ko_consonant_scan = self.ko_cho;
                commit_baseline = self.korean_state();
                commit_baseline.last_key = before.last_key;
                commit_baseline.vowel_toggle = before.vowel_toggle;
            } else {
                if let Some(committed) = Self::compose_korean_syllable(old_cho, old_jung, 1) {
                    Self::put_korean_char(&mut output.output0, &mut output.output0_len, committed);
                }

                self.reset_korean_composition();
                self.ko_cho = Self::korean_jong_to_cho(jong);
                self.ko_consonant_scan = self.ko_cho;
                commit_baseline = self.korean_state();
                commit_baseline.last_key = before.last_key;
                commit_baseline.vowel_toggle = before.vowel_toggle;
            }

            if !self.start_korean_vowel(scan) {
                return output;
            }
        } else if self.ko_jung.is_some() {
            if let Some((state, _)) = Self::korean_vowel_transition(self.ko_vowel_state, scan) {
                self.ko_vowel_state = state;
                self.ko_jung = Self::korean_vowel_jung(state);
            } else {
                if let Some(ch) = self.current_korean_char() {
                    Self::put_korean_char(&mut output.output0, &mut output.output0_len, ch);
                }

                self.reset_korean_composition();
                if !self.start_korean_vowel(scan) {
                    return output;
                }
            }
        } else if !self.start_korean_vowel(scan) {
            return InputMethodOutput::default();
        }

        if let Some(ch) = self.current_korean_char() {
            Self::put_korean_char(&mut output.output1, &mut output.output1_len, ch);
        }

        if output.output0_len != 0 {
            self.ko_undo.clear();
            self.ko_undo.push(commit_baseline);
        } else {
            self.ko_undo.push(before);
        }

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

#[cfg(test)]
mod korean_input_tests {
    use super::InputMethod;

    #[test]
    fn korean_mode_composes_and_commits_syllables() {
        let mut input = InputMethod::new();
        input.set_current_mode(3);

        let giyeok = input.handle_input(b'1' as i8, 2);
        assert!(giyeok.handled);
        assert_eq!(&giyeok.output1[..giyeok.output1_len], &[0xa4, 0xa1]);

        let ga = input.handle_input(b'3' as i8, 2);
        assert!(ga.handled);
        assert_eq!(&ga.output1[..ga.output1_len], &[0xb0, 0xa1]);

        let gak = input.handle_input(b'1' as i8, 2);
        assert!(gak.handled);
        assert_eq!(&gak.output1[..gak.output1_len], &[0xb0, 0xa2]);

        let split = input.handle_input(b'3' as i8, 2);
        assert!(split.handled);
        assert_eq!(&split.output0[..split.output0_len], &[0xb0, 0xa1]);
        assert_eq!(&split.output1[..split.output1_len], &[0xb0, 0xa1]);
    }

    #[test]
    fn korean_mode_applies_native_consonant_modifier() {
        let mut input = InputMethod::new();
        input.set_current_mode(3);

        input.handle_input(b'1' as i8, 2);
        let ssang = input.handle_input(b'#' as i8, 2);
        assert!(ssang.handled);
        assert_eq!(&ssang.output1[..ssang.output1_len], &[0xa4, 0xa2]);
    }

    #[test]
    fn korean_mode_flushes_composition_with_false_result() {
        let mut input = InputMethod::new();
        input.set_current_mode(3);

        input.handle_input(b'1' as i8, 2);
        input.handle_input(b'3' as i8, 2);

        let flush = input.handle_input(-99, 2);
        assert!(!flush.handled);
        assert_eq!(&flush.output0[..flush.output0_len], &[0xb0, 0xa1]);
        assert_eq!(flush.output1_len, 0);
    }

    #[test]
    fn korean_clear_removes_single_live_unit() {
        let mut input = InputMethod::new();
        input.set_current_mode(3);

        let giyeok = input.handle_input(b'1' as i8, 2);
        assert_eq!(&giyeok.output1[..giyeok.output1_len], &[0xa4, 0xa1]);

        let clear = input.handle_input(-16, 2);
        assert!(clear.handled);
        assert_eq!(clear.output0_len, 0);
        assert_eq!(clear.output1_len, 0);
    }

    #[test]
    fn korean_clear_restores_ga_from_gak() {
        let mut input = InputMethod::new();
        input.set_current_mode(3);

        input.handle_input(b'1' as i8, 2);
        input.handle_input(b'3' as i8, 2);
        let gak = input.handle_input(b'1' as i8, 2);
        assert_eq!(&gak.output1[..gak.output1_len], &[0xb0, 0xa2]);

        let clear = input.handle_input(-16, 2);
        assert!(clear.handled);
        assert_eq!(clear.output0_len, 0);
        assert_eq!(&clear.output1[..clear.output1_len], &[0xb0, 0xa1]);
    }

    #[test]
    fn korean_clear_after_jong_split_restores_inherited_cho() {
        let mut input = InputMethod::new();
        input.set_current_mode(3);

        input.handle_input(b'1' as i8, 2); // ㄱ
        input.handle_input(b'3' as i8, 2); // 가
        input.handle_input(b'1' as i8, 2); // 각

        let split = input.handle_input(b'3' as i8, 2);
        assert_eq!(&split.output0[..split.output0_len], &[0xb0, 0xa1]);
        assert_eq!(&split.output1[..split.output1_len], &[0xb0, 0xa1]);

        let clear = input.handle_input(-16, 2);
        assert!(clear.handled);
        assert_eq!(clear.output0_len, 0);
        assert_eq!(&clear.output1[..clear.output1_len], &[0xa4, 0xa1]);
    }

    #[test]
    fn korean_clear_does_not_cross_normal_commit_boundary() {
        let mut input = InputMethod::new();
        input.set_current_mode(3);

        input.handle_input(b'1' as i8, 2);
        let next = input.handle_input(b'2' as i8, 2);
        assert_ne!(next.output0_len, 0);

        let clear = input.handle_input(-16, 2);
        assert!(clear.handled);
        assert_eq!(clear.output0_len, 0);
        assert_eq!(clear.output1_len, 0);
    }
}

#[cfg(test)]
mod korean_scan_tests {
    use super::InputMethod;

    #[test]
    fn korean_characters_encode_as_euc_kr() {
        assert_eq!(InputMethod::encode_korean_char('가'), Some(([0xb0, 0xa1], 2)));
        assert_eq!(InputMethod::encode_korean_char('나'), Some(([0xb3, 0xaa], 2)));
        assert_eq!(InputMethod::encode_korean_char('A'), Some(([b'A', 0], 1)));
    }

    #[test]
    fn korean_internal_codes_compose_unicode_syllables() {
        assert_eq!(InputMethod::compose_korean_syllable(2, 3, 1), Some('가'));
        assert_eq!(InputMethod::compose_korean_syllable(4, 3, 1), Some('나'));
        assert_eq!(InputMethod::compose_korean_syllable(5, 3, 1), Some('다'));
        assert_eq!(InputMethod::compose_korean_syllable(2, 3, 2), Some('각'));
        assert_eq!(InputMethod::compose_korean_syllable(13, 29, 1), Some('이'));
        assert_eq!(InputMethod::compose_korean_syllable(20, 3, 4), Some('핛'));

        assert_eq!(InputMethod::compose_korean_syllable(1, 3, 1), None);
        assert_eq!(InputMethod::compose_korean_syllable(2, 2, 1), None);
        assert_eq!(InputMethod::compose_korean_syllable(2, 3, 18), None);
    }

    #[test]
    fn korean_jong_to_cho_matches_native_conversion() {
        let expected = [
            (2, 2),
            (3, 3),
            (5, 4),
            (8, 5),
            (9, 7),
            (17, 8),
            (19, 9),
            (21, 11),
            (22, 12),
            (23, 13),
            (24, 14),
            (25, 16),
            (26, 17),
            (27, 18),
            (28, 19),
            (29, 20),
        ];

        for (jong, cho) in expected {
            assert_eq!(InputMethod::korean_jong_to_cho(jong), Some(cho));
        }

        for jong in [4, 6, 7, 10, 11, 12, 13, 14, 15, 16, 20] {
            assert_eq!(InputMethod::korean_jong_to_cho(jong), None);
        }
    }

    #[test]
    fn korean_compound_jong_matches_native_table() {
        let expected = [
            ((2, 21), 4),
            ((5, 24), 6),
            ((5, 29), 7),
            ((9, 2), 10),
            ((9, 17), 11),
            ((9, 19), 12),
            ((9, 21), 13),
            ((9, 27), 14),
            ((9, 28), 15),
            ((9, 29), 16),
            ((19, 21), 20),
        ];

        for ((first, second), combined) in expected {
            assert_eq!(InputMethod::combine_korean_jong(first, second), Some(combined));
        }

        assert_eq!(InputMethod::combine_korean_jong(2, 2), None);
        assert_eq!(InputMethod::combine_korean_jong(5, 21), None);
        assert_eq!(InputMethod::combine_korean_jong(17, 21), None);
    }

    #[test]
    fn korean_consonant_scans_match_native_jong_codes() {
        let expected = [
            (2, 2),
            (3, 3),
            (4, 5),
            (5, 8),
            (7, 9),
            (8, 17),
            (9, 19),
            (11, 21),
            (12, 22),
            (13, 23),
            (14, 24),
            (16, 25),
            (17, 26),
            (18, 27),
            (19, 28),
            (20, 29),
        ];

        for (scan, jong) in expected {
            assert_eq!(InputMethod::korean_scan_to_jong(scan), Some(jong));
        }

        assert_eq!(InputMethod::korean_scan_to_jong(6), None);
        assert_eq!(InputMethod::korean_scan_to_jong(10), None);
        assert_eq!(InputMethod::korean_scan_to_jong(15), None);
    }

    #[test]
    fn korean_vowel_states_match_native_jung_codes() {
        let expected = [
            (1, 3),
            (2, 5),
            (3, 6),
            (4, 4),
            (5, 7),
            (6, 11),
            (7, 12),
            (8, 10),
            (9, 13),
            (10, 19),
            (11, 18),
            (12, 14),
            (13, 15),
            (14, 20),
            (15, 26),
            (16, 21),
            (17, 22),
            (18, 23),
            (19, 27),
            (20, 28),
            (21, 29),
        ];

        for (state, jung) in expected {
            assert_eq!(InputMethod::korean_vowel_jung(state), Some(jung));
        }

        assert_eq!(InputMethod::korean_vowel_jung(0), None);
        assert_eq!(InputMethod::korean_vowel_jung(22), None);
    }

    #[test]
    fn korean_vowel_fst_matches_native_transitions() {
        assert_eq!(InputMethod::korean_vowel_transition(0, 21), Some((1, false)));
        assert_eq!(InputMethod::korean_vowel_transition(0, 23), Some((9, false)));
        assert_eq!(InputMethod::korean_vowel_transition(0, 25), Some((19, false)));
        assert_eq!(InputMethod::korean_vowel_transition(0, 26), Some((21, false)));

        assert_eq!(InputMethod::korean_vowel_transition(1, 21), Some((5, true)));
        assert_eq!(InputMethod::korean_vowel_transition(1, 26), Some((4, false)));
        assert_eq!(InputMethod::korean_vowel_transition(1, 27), Some((2, false)));

        assert_eq!(InputMethod::korean_vowel_transition(2, 26), Some((3, false)));
        assert_eq!(InputMethod::korean_vowel_transition(2, 27), Some((1, false)));

        assert_eq!(InputMethod::korean_vowel_transition(5, 22), Some((1, true)));
        assert_eq!(InputMethod::korean_vowel_transition(5, 26), Some((8, false)));
        assert_eq!(InputMethod::korean_vowel_transition(5, 28), Some((6, false)));

        assert_eq!(InputMethod::korean_vowel_transition(6, 26), Some((7, false)));
        assert_eq!(InputMethod::korean_vowel_transition(6, 27), Some((5, false)));

        assert_eq!(InputMethod::korean_vowel_transition(9, 21), Some((12, false)));
        assert_eq!(InputMethod::korean_vowel_transition(9, 23), Some((14, true)));
        assert_eq!(InputMethod::korean_vowel_transition(9, 26), Some((11, false)));
        assert_eq!(InputMethod::korean_vowel_transition(9, 28), Some((10, false)));
        assert_eq!(InputMethod::korean_vowel_transition(9, 25), None);

        assert_eq!(InputMethod::korean_vowel_transition(10, 27), Some((9, true)));
        assert_eq!(InputMethod::korean_vowel_transition(12, 26), Some((13, false)));

        assert_eq!(InputMethod::korean_vowel_transition(14, 21), Some((16, false)));
        assert_eq!(InputMethod::korean_vowel_transition(14, 24), Some((9, true)));
        assert_eq!(InputMethod::korean_vowel_transition(14, 26), Some((18, false)));
        assert_eq!(InputMethod::korean_vowel_transition(14, 27), Some((15, false)));
        assert_eq!(InputMethod::korean_vowel_transition(14, 25), None);

        assert_eq!(InputMethod::korean_vowel_transition(15, 27), Some((14, false)));
        assert_eq!(InputMethod::korean_vowel_transition(16, 26), Some((17, false)));

        assert_eq!(InputMethod::korean_vowel_transition(19, 26), Some((20, true)));
        assert_eq!(InputMethod::korean_vowel_transition(19, 27), Some((20, false)));
        assert_eq!(InputMethod::korean_vowel_transition(19, 28), Some((20, false)));

        assert_eq!(InputMethod::korean_vowel_transition(20, 26), None);
    }

    #[test]
    fn korean_initial_key_scans_match_native_mapping() {
        assert_eq!(InputMethod::initial_korean_scan(b'1' as i8), Some(2));
        assert_eq!(InputMethod::initial_korean_scan(b'2' as i8), Some(4));
        assert_eq!(InputMethod::initial_korean_scan(b'3' as i8), Some(21));
        assert_eq!(InputMethod::initial_korean_scan(b'4' as i8), Some(7));
        assert_eq!(InputMethod::initial_korean_scan(b'5' as i8), Some(8));
        assert_eq!(InputMethod::initial_korean_scan(b'6' as i8), Some(23));
        assert_eq!(InputMethod::initial_korean_scan(b'7' as i8), Some(11));
        assert_eq!(InputMethod::initial_korean_scan(b'8' as i8), Some(13));
        assert_eq!(InputMethod::initial_korean_scan(b'9' as i8), Some(25));
        assert_eq!(InputMethod::initial_korean_scan(b'0' as i8), Some(26));

        assert_eq!(InputMethod::initial_korean_scan(b'*' as i8), None);
        assert_eq!(InputMethod::initial_korean_scan(b'#' as i8), None);
    }

    #[test]
    fn korean_consonant_modifiers_match_native_kscan() {
        assert_eq!(InputMethod::modify_korean_consonant(2, 42), Some(17));
        assert_eq!(InputMethod::modify_korean_consonant(2, 35), Some(3));
        assert_eq!(InputMethod::modify_korean_consonant(3, 35), Some(2));
        assert_eq!(InputMethod::modify_korean_consonant(17, 42), Some(2));

        assert_eq!(InputMethod::modify_korean_consonant(4, 42), Some(5));
        assert_eq!(InputMethod::modify_korean_consonant(5, 42), Some(18));
        assert_eq!(InputMethod::modify_korean_consonant(5, 35), Some(6));
        assert_eq!(InputMethod::modify_korean_consonant(6, 35), Some(5));
        assert_eq!(InputMethod::modify_korean_consonant(18, 42), Some(4));

        assert_eq!(InputMethod::modify_korean_consonant(8, 42), Some(9));
        assert_eq!(InputMethod::modify_korean_consonant(9, 42), Some(19));
        assert_eq!(InputMethod::modify_korean_consonant(9, 35), Some(10));
        assert_eq!(InputMethod::modify_korean_consonant(10, 35), Some(9));
        assert_eq!(InputMethod::modify_korean_consonant(19, 42), Some(8));

        assert_eq!(InputMethod::modify_korean_consonant(11, 42), Some(14));
        assert_eq!(InputMethod::modify_korean_consonant(11, 35), Some(12));
        assert_eq!(InputMethod::modify_korean_consonant(12, 35), Some(11));
        assert_eq!(InputMethod::modify_korean_consonant(14, 42), Some(16));
        assert_eq!(InputMethod::modify_korean_consonant(14, 35), Some(15));
        assert_eq!(InputMethod::modify_korean_consonant(15, 35), Some(14));
        assert_eq!(InputMethod::modify_korean_consonant(16, 42), Some(11));

        assert_eq!(InputMethod::modify_korean_consonant(13, 42), Some(20));
        assert_eq!(InputMethod::modify_korean_consonant(20, 42), Some(13));

        assert_eq!(InputMethod::modify_korean_consonant(7, 42), None);
        assert_eq!(InputMethod::modify_korean_consonant(7, 35), None);
        assert_eq!(InputMethod::modify_korean_consonant(4, 35), None);
    }
}
