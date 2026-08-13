use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{Array, ClassInstanceRef, JavaChar, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.TextFormatProcessor
pub struct TextFormatProcessor;

impl TextFormatProcessor {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextFormatProcessor",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("getCurLine", "()I", Self::get_cur_line, Default::default()),
                JavaMethodProto::new(
                    "getFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    Self::get_font,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setConstraints",
                    "(I)V",
                    Self::set_constraints,
                    Default::default(),
                ),
                JavaMethodProto::new("getData", "()[C", Self::get_data, Default::default()),
                JavaMethodProto::new(
                    "getDataHeight",
                    "()I",
                    Self::get_data_height,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setLineWidth",
                    "(I)V",
                    Self::set_line_width,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setData",
                    "([C)I",
                    Self::set_data,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setData",
                    "([CI)I",
                    Self::set_data_with_position,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setWHSize",
                    "(II)I",
                    Self::set_wh_size,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setWHLSize",
                    "(III)I",
                    Self::set_whl_size,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setCurrent",
                    "(I)V",
                    Self::set_current,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getUpDownPosition",
                    "(II)I",
                    Self::get_up_down_position,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "paintChar",
                    "(Lorg/kwis/msp/lcdui/Graphics;IIZ)V",
                    Self::paint_char,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "paintChar",
                    "(Lorg/kwis/msp/lcdui/Graphics;Z)V",
                    Self::paint_char_default,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setFont",
                    "(Lorg/kwis/msp/lcdui/Font;)V",
                    Self::set_font,
                    Default::default(),
                ),
            ],
            fields: vec![
                // WipiPlayer Plus native TextFormatProcessor instance state.
                // Synthetic fields preserve native per-instance semantics.
                JavaFieldProto::new("__wieState", "I", Default::default()),           // +0x00
                JavaFieldProto::new("__wieWidth", "I", Default::default()),           // +0x04
                JavaFieldProto::new("__wieLineWidth", "I", Default::default()),       // +0x08
                JavaFieldProto::new("__wieHeight", "I", Default::default()),          // +0x0c
                JavaFieldProto::new("__wieDataHeight", "I", Default::default()),      // +0x10
                JavaFieldProto::new("__wieData", "[C", Default::default()),           // +0x14
                JavaFieldProto::new("__wieCurLine", "I", Default::default()),         // +0x18
                JavaFieldProto::new("__wieLinePositions", "[I", Default::default()),  // +0x1c
                JavaFieldProto::new("__wieLineCount", "I", Default::default()),       // +0x20
                JavaFieldProto::new(
                    "__wieFont",
                    "Lorg/kwis/msp/lcdui/Font;",
                    Default::default(),
                ),                                                                    // +0x24
                JavaFieldProto::new("__wieFontHeight", "I", Default::default()),      // +0x28
                JavaFieldProto::new("__wieDataLength", "I", Default::default()),      // +0x2c
                JavaFieldProto::new("__wieCurrent", "I", Default::default()),         // +0x30
                JavaFieldProto::new("__wieConstraints", "I", Default::default()),     // +0x34
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFormatProcessor>,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "java/lang/Object", "<init>", "()V", ())
            .await?;

        // Native +0x00 starts at 1.
        jvm.put_field(&mut this, "__wieState", "I", 1).await?;

        // Native constructor creates int[1], stores 0 at index 0, and keeps
        // that array as the initial line-position table (+0x1c).
        let mut line_positions: ClassInstanceRef<Array<i32>> =
            jvm.instantiate_array("I", 1).await?.into();
        jvm.store_array(&mut line_positions, 0, [0i32]).await?;
        jvm.put_field(
            &mut this,
            "__wieLinePositions",
            "[I",
            line_positions,
        )
        .await?;

        // Native logical line count starts at 1 and cursor/current at -1.
        jvm.put_field(&mut this, "__wieLineCount", "I", 1).await?;
        jvm.put_field(&mut this, "__wieCurrent", "I", -1).await?;

        let font: ClassInstanceRef<()> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Font",
                "getDefaultFont",
                "()Lorg/kwis/msp/lcdui/Font;",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "__wieFont",
            "Lorg/kwis/msp/lcdui/Font;",
            font.clone(),
        )
        .await?;

        let height: i32 = jvm
            .invoke_virtual(&font, "getHeight", "()I", ())
            .await?;
        jvm.put_field(
            &mut this,
            "__wieFontHeight",
            "I",
            height.wrapping_add(2),
        )
        .await?;

        Ok(())
    }

    async fn get_cur_line(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
    ) -> JvmResult<i32> {
        jvm.get_field(&this, "__wieCurLine", "I").await
    }

    async fn get_font(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
    ) -> JvmResult<ClassInstanceRef<()>> {
        jvm.get_field(
            &this,
            "__wieFont",
            "Lorg/kwis/msp/lcdui/Font;",
        )
        .await
    }

    async fn set_constraints(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        constraints: i32,
    ) -> JvmResult<()> {
        jvm.put_field(
            &mut this,
            "__wieConstraints",
            "I",
            constraints,
        )
        .await
    }

    async fn get_data(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
    ) -> JvmResult<ClassInstanceRef<Array<JavaChar>>> {
        jvm.get_field(&this, "__wieData", "[C").await
    }

    async fn get_data_height(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
    ) -> JvmResult<i32> {
        jvm.get_field(&this, "__wieDataHeight", "I").await
    }

    async fn set_line_width(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        width: i32,
    ) -> JvmResult<()> {
        jvm.put_field(&mut this, "__wieLineWidth", "I", width).await
    }

    async fn set_data(
        jvm: &Jvm,
        ctx: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
        data: ClassInstanceRef<Array<JavaChar>>,
    ) -> JvmResult<i32> {
        Self::set_data_with_position(jvm, ctx, this, data, 0).await
    }

    async fn set_data_with_position(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        data: ClassInstanceRef<Array<JavaChar>>,
        position: i32,
    ) -> JvmResult<i32> {
        if data.is_null() {
            return Ok(0);
        }

        let length = jvm.array_length(&data).await? as i32;
        let position = position.max(0);

        jvm.put_field(&mut this, "__wieData", "[C", data).await?;
        jvm.put_field(&mut this, "__wieDataLength", "I", length).await?;

        Self::rebuild(jvm, this, position).await
    }

    async fn set_wh_size(
        jvm: &Jvm,
        ctx: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
        width: i32,
        height: i32,
    ) -> JvmResult<i32> {
        Self::set_whl_size(
            jvm,
            ctx,
            this,
            width,
            height,
            width,
        )
        .await
    }

    async fn set_whl_size(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        width: i32,
        height: i32,
        line_width: i32,
    ) -> JvmResult<i32> {
        jvm.put_field(&mut this, "__wieLineWidth", "I", line_width).await?;
        jvm.put_field(&mut this, "__wieWidth", "I", width).await?;
        jvm.put_field(&mut this, "__wieHeight", "I", height).await?;

        Self::rebuild(jvm, this, 0).await
    }

    async fn rebuild(
        jvm: &Jvm,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        start_position: i32,
    ) -> JvmResult<i32> {
        let width: i32 = jvm.get_field(&this, "__wieWidth", "I").await?;
        let line_width: i32 = jvm.get_field(&this, "__wieLineWidth", "I").await?;
        let font_height: i32 = jvm.get_field(&this, "__wieFontHeight", "I").await?;

        if width == -1 {
            let font: ClassInstanceRef<()> = jvm
                .get_field(
                    &this,
                    "__wieFont",
                    "Lorg/kwis/msp/lcdui/Font;",
                )
                .await?;

            let height: i32 = jvm
                .invoke_virtual(&font, "getHeight", "()I", ())
                .await?;

            jvm.put_field(&mut this, "__wieDataHeight", "I", height).await?;
            return Ok(height);
        }

        let data: ClassInstanceRef<Array<JavaChar>> =
            jvm.get_field(&this, "__wieData", "[C").await?;
        let data_length: i32 = jvm.get_field(&this, "__wieDataLength", "I").await?;

        if width <= 0 || line_width <= 0 || data.is_null() || data_length <= 0 {
            return Ok(font_height);
        }

        let chars: alloc::vec::Vec<JavaChar> =
            jvm.load_array(&data, 0, data_length as usize).await?;

        let font: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "__wieFont",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        let mut line_number = 1i32;
        let mut scan_start = 0i32;
        let mut accumulated_width = 0i32;

        // Native rebuild(start_position) preserves the already-valid prefix
        // and resumes from the beginning of the logical line containing the
        // changed position.
        if start_position != 0 {
            let old_line_count: i32 =
                jvm.get_field(&this, "__wieLineCount", "I").await?;

            if old_line_count > 1 {
                let positions: ClassInstanceRef<Array<i32>> =
                    jvm.get_field(&this, "__wieLinePositions", "[I").await?;
                let capacity = jvm.array_length(&positions).await? as i32;
                let usable = core::cmp::min(old_line_count, capacity);

                if usable > 0 {
                    let old_positions: alloc::vec::Vec<i32> =
                        jvm.load_array(&positions, 0, usable as usize).await?;

                    let mut containing_line = 0usize;

                    for line in 1..old_positions.len() {
                        if old_positions[line] > start_position {
                            break;
                        }
                        containing_line = line;
                    }

                    scan_start = old_positions[containing_line]
                        .max(0)
                        .min(data_length);
                    line_number = containing_line as i32 + 1;
                }
            }
        }

        for index in scan_start..data_length {
            let ch = chars[index as usize];

            let char_width: i32 = jvm
                .invoke_virtual(
                    &font,
                    "charWidth",
                    "(C)I",
                    (ch,),
                )
                .await?;

            accumulated_width = accumulated_width.wrapping_add(char_width);

            if ch as u16 == 10 {
                line_number += 1;
                let next = index + 1;
                Self::mark_new_line_position(
                    jvm,
                    this.clone(),
                    next,
                    line_number,
                )
                .await?;
                accumulated_width = 0;
                continue;
            }

            if accumulated_width > line_width {
                line_number += 1;

                // Native wraps before the character that exceeded the width,
                // so this character becomes the first character of the new line.
                Self::mark_new_line_position(
                    jvm,
                    this.clone(),
                    index,
                    line_number,
                )
                .await?;

                accumulated_width = char_width;
            }
        }

        let data_height = line_number.wrapping_mul(font_height);

        jvm.put_field(&mut this, "__wieLineCount", "I", line_number)
            .await?;
        jvm.put_field(&mut this, "__wieDataHeight", "I", data_height)
            .await?;

        Ok(data_height)
    }

    async fn mark_new_line_position(
        jvm: &Jvm,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        position: i32,
        line_number: i32,
    ) -> JvmResult<()> {
        let mut positions: ClassInstanceRef<Array<i32>> =
            jvm.get_field(&this, "__wieLinePositions", "[I").await?;

        let capacity = jvm.array_length(&positions).await? as i32;

        // Native grows the backing int[] in fixed chunks of four entries
        // whenever the requested 1-based line number exceeds capacity.
        if line_number > capacity {
            let new_capacity = capacity + 4;
            let mut expanded: ClassInstanceRef<Array<i32>> =
                jvm.instantiate_array("I", new_capacity as usize).await?.into();

            if capacity > 0 {
                let old_values: alloc::vec::Vec<i32> =
                    jvm.load_array(&positions, 0, capacity as usize).await?;
                jvm.store_array(&mut expanded, 0, old_values).await?;
            }

            jvm.put_field(
                &mut this,
                "__wieLinePositions",
                "[I",
                expanded.clone(),
            )
            .await?;

            positions = expanded;
        }

        // Native line numbers are 1-based while the backing int[] is 0-based.
        let index = line_number - 1;
        jvm.store_array(&mut positions, index as usize, [position]).await?;

        Ok(())
    }

    async fn find_new_position(
        jvm: &Jvm,
        this: &ClassInstanceRef<TextFormatProcessor>,
        target_line_start: i32,
        current_position: i32,
        current_line_start: i32,
    ) -> JvmResult<i32> {
        let data: ClassInstanceRef<Array<JavaChar>> =
            jvm.get_field(this, "__wieData", "[C").await?;
        let data_length: i32 =
            jvm.get_field(this, "__wieDataLength", "I").await?;
        let font: ClassInstanceRef<()> = jvm
            .get_field(
                this,
                "__wieFont",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        // Native computes the horizontal pixel offset of the cursor within
        // its current logical line.
        let mut target_x = 0i32;

        if current_position > current_line_start
            && current_line_start < data_length
        {
            let start = current_line_start.max(0);
            let end = current_position.min(data_length);

            if end > start {
                let chars: alloc::vec::Vec<JavaChar> = jvm
                    .load_array(
                        &data,
                        start as usize,
                        (end - start) as usize,
                    )
                    .await?;

                for ch in chars {
                    let width: i32 = jvm
                        .invoke_virtual(
                            &font,
                            "charWidth",
                            "(C)I",
                            (ch,),
                        )
                        .await?;
                    target_x = target_x.wrapping_add(width);
                }
            }
        }

        if target_line_start >= data_length {
            return Ok(target_line_start);
        }

        let target_line = Self::find_line(jvm, this, target_line_start).await?;
        if target_line < 0 {
            return Ok(target_line_start);
        }

        let mut position = target_line_start.max(0);
        let mut x = 0i32;

        while position < data_length {
            // Do not walk past the target logical line.
            if Self::find_line(jvm, this, position).await? != target_line {
                return Ok(position - 1);
            }

            let chars: alloc::vec::Vec<JavaChar> =
                jvm.load_array(&data, position as usize, 1).await?;
            let ch = chars[0];

            // A newline terminates the logical line; the cursor remains
            // immediately before it.
            if ch as u16 == 10 {
                return Ok(position);
            }

            let width: i32 = jvm
                .invoke_virtual(
                    &font,
                    "charWidth",
                    "(C)I",
                    (ch,),
                )
                .await?;

            let next_x = x.wrapping_add(width);

            // Native advances while the next character still fits at the
            // requested horizontal offset, otherwise returns this position.
            if next_x > target_x {
                return Ok(position);
            }

            x = next_x;
            position += 1;
        }

        Ok(position)
    }

    async fn find_line(
        jvm: &Jvm,
        this: &ClassInstanceRef<TextFormatProcessor>,
        position: i32,
    ) -> JvmResult<i32> {
        let line_count: i32 =
            jvm.get_field(this, "__wieLineCount", "I").await?;

        if line_count <= 0 {
            return Ok(-1);
        }

        let positions: ClassInstanceRef<Array<i32>> =
            jvm.get_field(this, "__wieLinePositions", "[I").await?;

        let capacity = jvm.array_length(&positions).await?;
        if capacity == 0 {
            return Ok(-1);
        }

        let values: alloc::vec::Vec<i32> =
            jvm.load_array(&positions, 0, capacity).await?;

        if values[0] > position {
            return Ok(-1);
        }

        let usable = core::cmp::min(line_count as usize, values.len());

        for line in 1..usable {
            if values[line] > position {
                return Ok(line as i32 - 1);
            }
        }

        Ok(line_count - 1)
    }

    async fn set_current(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        position: i32,
    ) -> JvmResult<()> {
        let mut position = position.max(0);

        let data: ClassInstanceRef<Array<JavaChar>> =
            jvm.get_field(&this, "__wieData", "[C").await?;

        // Native dereferences the data array here; array_length preserves
        // the same null-array failure rather than silently accepting it.
        let data_length = jvm.array_length(&data).await? as i32;

        if position >= data_length {
            position = data_length;
        }

        jvm.put_field(&mut this, "__wieCurrent", "I", position)
            .await?;

        let line = Self::find_line(jvm, &this, position).await?;
        jvm.put_field(&mut this, "__wieCurLine", "I", line)
            .await?;

        Ok(())
    }

    async fn paint_char_default(
        jvm: &Jvm,
        ctx: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
        graphics: ClassInstanceRef<()>,
        draw_caret: bool,
    ) -> JvmResult<()> {
        Self::paint_char(
            jvm,
            ctx,
            this,
            graphics,
            0,
            0,
            draw_caret,
        )
        .await
    }

    async fn paint_char(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
        graphics: ClassInstanceRef<()>,
        start_line: i32,
        visual_line: i32,
        draw_caret: bool,
    ) -> JvmResult<()> {
        let _: () = jvm.invoke_virtual(
            &graphics,
            "reset",
            "()V",
            (),
        )
        .await?;

        let width: i32 =
            jvm.get_field(&this, "__wieWidth", "I").await?;
        let data: ClassInstanceRef<Array<JavaChar>> =
            jvm.get_field(&this, "__wieData", "[C").await?;
        let data_length: i32 =
            jvm.get_field(&this, "__wieDataLength", "I").await?;
        let line_count: i32 =
            jvm.get_field(&this, "__wieLineCount", "I").await?;
        let font_height: i32 =
            jvm.get_field(&this, "__wieFontHeight", "I").await?;
        let current: i32 =
            jvm.get_field(&this, "__wieCurrent", "I").await?;
        let constraints: i32 =
            jvm.get_field(&this, "__wieConstraints", "I").await?;

        let font: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "__wieFont",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        if width <= 0 || data.is_null() || data_length == 0 {
            if draw_caret {
                let _: () = jvm.invoke_virtual(
                    &graphics,
                    "drawLine",
                    "(IIII)V",
                    (1i32, 0i32, 1i32, font_height),
                )
                .await?;
            }
            return Ok(());
        }

        if start_line < 0
            || visual_line < 0
            || start_line >= line_count
            || visual_line >= line_count
        {
            return Ok(());
        }

        let positions: ClassInstanceRef<Array<i32>> =
            jvm.get_field(&this, "__wieLinePositions", "[I").await?;
        let positions_len = jvm.array_length(&positions).await? as i32;

        if start_line >= positions_len {
            return Ok(());
        }

        let mut line = start_line;
        let mut screen_line = visual_line;
        let mut last_x = 1i32;
        let mut last_y = screen_line.wrapping_mul(font_height);

        while line < line_count
            && screen_line < line_count
            && line < positions_len
        {
            let start_vec: alloc::vec::Vec<i32> =
                jvm.load_array(&positions, line as usize, 1).await?;
            let line_start = start_vec[0];

            let line_end = if line + 1 < line_count
                && line + 1 < positions_len
            {
                let end_vec: alloc::vec::Vec<i32> =
                    jvm.load_array(
                        &positions,
                        (line + 1) as usize,
                        1,
                    )
                    .await?;
                end_vec[0]
            } else {
                data_length
            };

            let mut x = 1i32;
            let y = screen_line.wrapping_mul(font_height);
            let start = line_start.max(0).min(data_length);
            let end = line_end.max(start).min(data_length);

            let mut index = start;
            while index < end {
                let chars: alloc::vec::Vec<JavaChar> =
                    jvm.load_array(&data, index as usize, 1).await?;
                let ch = chars[0];

                if ch as u16 != 10 {
                    let draw_ch: JavaChar = if constraints == 2 {
                        42u16
                    } else {
                        ch
                    };

                    let _: () = jvm.invoke_virtual(
                        &graphics,
                        "drawChar",
                        "(CIII)V",
                        (draw_ch, x, y, 4i32),
                    )
                    .await?;

                    if draw_caret && index == current {
                        let _: () = jvm.invoke_virtual(
                            &graphics,
                            "drawLine",
                            "(IIII)V",
                            (
                                x,
                                y,
                                x,
                                y.wrapping_add(font_height),
                            ),
                        )
                        .await?;
                    }

                    let char_width: i32 = jvm
                        .invoke_virtual(
                            &font,
                            "charWidth",
                            "(C)I",
                            (ch,),
                        )
                        .await?;

                    x = x.wrapping_add(char_width);
                } else if draw_caret && index == current {
                    let _: () = jvm.invoke_virtual(
                        &graphics,
                        "drawLine",
                        "(IIII)V",
                        (
                            x,
                            y,
                            x,
                            y.wrapping_add(font_height),
                        ),
                    )
                    .await?;
                }

                index += 1;
            }

            last_x = x;
            last_y = y;

            line += 1;
            screen_line += 1;
        }

        if draw_caret && current >= data_length {
            let _: () = jvm.invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (
                    last_x,
                    last_y,
                    last_x,
                    last_y.wrapping_add(font_height),
                ),
            )
            .await?;
        }

        Ok(())
    }

    async fn get_up_down_position(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFormatProcessor>,
        position: i32,
        direction: i32,
    ) -> JvmResult<i32> {
        let current_line = Self::find_line(jvm, &this, position).await?;

        if direction != 1 && direction != -1 {
            return Ok(position);
        }

        let line_count: i32 =
            jvm.get_field(&this, "__wieLineCount", "I").await?;

        if current_line < 0 || line_count <= 0 {
            return Ok(-1);
        }

        let target_line = if direction == 1 {
            if current_line >= line_count - 1 {
                return Ok(-1);
            }
            current_line + 1
        } else {
            if current_line <= 0 {
                return Ok(-1);
            }
            current_line - 1
        };

        let positions: ClassInstanceRef<Array<i32>> =
            jvm.get_field(&this, "__wieLinePositions", "[I").await?;

        let capacity = jvm.array_length(&positions).await? as i32;

        if current_line >= capacity || target_line >= capacity {
            return Ok(-1);
        }

        let current_start: alloc::vec::Vec<i32> =
            jvm.load_array(&positions, current_line as usize, 1).await?;
        let target_start: alloc::vec::Vec<i32> =
            jvm.load_array(&positions, target_line as usize, 1).await?;

        Self::find_new_position(
            jvm,
            &this,
            target_start[0],
            position,
            current_start[0],
        )
        .await
    }

    async fn set_font(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFormatProcessor>,
        font: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        jvm.put_field(
            &mut this,
            "__wieFont",
            "Lorg/kwis/msp/lcdui/Font;",
            font.clone(),
        )
        .await?;

        let height: i32 = jvm
            .invoke_virtual(&font, "getHeight", "()I", ())
            .await?;

        jvm.put_field(
            &mut this,
            "__wieFontHeight",
            "I",
            height.wrapping_add(2),
        )
        .await?;

        let _ = Self::rebuild(jvm, this, 0).await?;

        Ok(())
    }
}
