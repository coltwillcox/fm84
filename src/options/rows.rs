//! The rows of F11, and what each shows of the options and changes in them.

use super::*;

/// The rows of the options popup, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionRow {
    ShowHidden,
    SortKey,
    SortDirection,
    DirsFirst,
    CaseSensitive,
    ColumnExt,
    ColumnSize,
    ColumnModified,
    ColumnAttributes,
    GitMarks,
    DateFormat,
    SizeUnits,
    ConfirmDelete,
    DeleteToTrash,
    ConfirmCopyMove,
    OnExisting,
    Terminal,
    Editor,
    RememberDirs,
    PreviewOnStart,
    Language,
    Theme,
    ThemeBackground,
    IconStyle,
    Clock,
    Cursor,
    CursorBlink,
    TabWidth,
    LineNumbers,
    ScrollBars,
    HighlightLimit,
    LargeFile,
    ImageDefault,
    ImageBackgrounds,
    ImagePrefetch,
}

pub const OPTION_ROWS: [OptionRow; 35] = [
    OptionRow::ShowHidden,
    OptionRow::SortKey,
    OptionRow::SortDirection,
    OptionRow::DirsFirst,
    OptionRow::CaseSensitive,
    OptionRow::ColumnExt,
    OptionRow::ColumnSize,
    OptionRow::ColumnModified,
    OptionRow::ColumnAttributes,
    OptionRow::GitMarks,
    OptionRow::DateFormat,
    OptionRow::SizeUnits,
    OptionRow::ConfirmDelete,
    OptionRow::DeleteToTrash,
    OptionRow::ConfirmCopyMove,
    OptionRow::OnExisting,
    OptionRow::Terminal,
    OptionRow::Editor,
    OptionRow::RememberDirs,
    OptionRow::PreviewOnStart,
    OptionRow::Language,
    OptionRow::Theme,
    OptionRow::ThemeBackground,
    OptionRow::IconStyle,
    OptionRow::Clock,
    OptionRow::Cursor,
    OptionRow::CursorBlink,
    OptionRow::TabWidth,
    OptionRow::LineNumbers,
    OptionRow::ScrollBars,
    OptionRow::HighlightLimit,
    OptionRow::LargeFile,
    OptionRow::ImageDefault,
    OptionRow::ImageBackgrounds,
    OptionRow::ImagePrefetch,
];

impl OptionRow {
    pub fn label(self) -> &'static str {
        match self {
            OptionRow::ShowHidden => t!("Show hidden files"),
            OptionRow::SortKey => t!("Sort by"),
            OptionRow::SortDirection => t!("Sort direction"),
            OptionRow::DirsFirst => t!("Directories first"),
            OptionRow::CaseSensitive => t!("Case-sensitive sort"),
            OptionRow::ColumnExt => t!("Ext column"),
            OptionRow::ColumnSize => t!("Size column"),
            OptionRow::ColumnModified => t!("Modified column"),
            OptionRow::ColumnAttributes => t!("Attributes column"),
            OptionRow::GitMarks => t!("Git marks"),
            OptionRow::DateFormat => t!("Date format"),
            OptionRow::SizeUnits => t!("Size units"),
            OptionRow::ConfirmDelete => t!("Confirm delete"),
            OptionRow::DeleteToTrash => t!("Delete to trash (F8)"),
            OptionRow::ConfirmCopyMove => t!("Confirm copy and move"),
            OptionRow::OnExisting => t!("When destination exists"),
            OptionRow::Terminal => t!("Terminal (F9)"),
            OptionRow::Editor => t!("Editor (F4)"),
            OptionRow::RememberDirs => t!("Remember directories"),
            OptionRow::PreviewOnStart => t!("Preview on startup"),
            OptionRow::Language => t!("Language"),
            OptionRow::Theme => t!("Theme"),
            OptionRow::ThemeBackground => t!("Background"),
            OptionRow::IconStyle => t!("Icons"),
            OptionRow::Clock => t!("Clock"),
            OptionRow::Cursor => t!("Cursor"),
            OptionRow::CursorBlink => t!("Cursor blinks"),
            OptionRow::TabWidth => t!("Tab width"),
            OptionRow::LineNumbers => t!("Line numbers"),
            OptionRow::ScrollBars => t!("Scroll bars"),
            OptionRow::HighlightLimit => t!("Highlight files up to"),
            OptionRow::LargeFile => t!("Ask before opening over"),
            OptionRow::ImageDefault => t!("Images open as"),
            OptionRow::ImageBackgrounds => t!("Image backgrounds"),
            OptionRow::ImagePrefetch => t!("Read pictures ahead"),
        }
    }

    /// The heading this row opens, when it is the first of its group.
    pub fn section(self) -> Option<&'static str> {
        match self {
            OptionRow::ShowHidden => Some(t!("Panels")),
            OptionRow::ConfirmDelete => Some(t!("Behaviour")),
            OptionRow::Language => Some(t!("Appearance")),
            OptionRow::TabWidth => Some(t!("Viewer and editor")),
            _ => None,
        }
    }

    /// True for a row edited by typing rather than by stepping through values.
    pub fn is_text(self) -> bool {
        matches!(self, OptionRow::Terminal | OptionRow::Editor)
    }

    /// Whether changing this row changes what the panels hold, rather than
    /// only how they are drawn. The sizes are written out as the rows load.
    pub fn affects_listing(self) -> bool {
        matches!(
            self,
            OptionRow::ShowHidden | OptionRow::SortKey | OptionRow::SortDirection | OptionRow::DirsFirst | OptionRow::CaseSensitive | OptionRow::SizeUnits | OptionRow::GitMarks
        )
    }
}

impl Options {
    pub fn value(&self, row: OptionRow) -> String {
        let on_off = |on: bool| if on { t!("On") } else { t!("Off") }.to_string();
        match row {
            OptionRow::ShowHidden => on_off(self.show_hidden),
            OptionRow::SortKey => match self.sort_key {
                SortKey::Name => t!("Name"),
                SortKey::Extension => t!("Extension"),
                SortKey::Size => t!("Size"),
                SortKey::Modified => t!("Modified"),
            }
            .to_string(),
            OptionRow::SortDirection => if self.sort_descending { t!("Descending") } else { t!("Ascending") }.to_string(),
            OptionRow::DirsFirst => on_off(self.dirs_first),
            OptionRow::CaseSensitive => on_off(self.case_sensitive),
            OptionRow::ColumnExt => on_off(self.column_ext),
            OptionRow::ColumnSize => on_off(self.column_size),
            OptionRow::ColumnModified => on_off(self.column_modified),
            OptionRow::ColumnAttributes => on_off(self.column_attributes),
            OptionRow::GitMarks => on_off(self.git_marks),
            OptionRow::DateFormat => match self.date_format {
                DateFormat::Short => "dd/mm/yy",
                DateFormat::Iso => "yyyy-mm-dd",
                DateFormat::Relative => t!("Relative"),
            }
            .to_string(),
            OptionRow::SizeUnits => match self.size_units {
                SizeUnits::Binary => "KiB (1024)",
                SizeUnits::Decimal => "kB (1000)",
            }
            .to_string(),
            OptionRow::ConfirmDelete => on_off(self.confirm_delete),
            OptionRow::DeleteToTrash => on_off(self.delete_to_trash),
            OptionRow::ConfirmCopyMove => on_off(self.confirm_copy_move),
            OptionRow::OnExisting => match self.on_existing {
                OnExisting::Ask => t!("Ask"),
                OnExisting::Overwrite => t!("Overwrite"),
                OnExisting::Refuse => t!("Refuse"),
                OnExisting::KeepBoth => t!("Keep both"),
            }
            .to_string(),
            OptionRow::Terminal if self.terminal.is_empty() => t!("Automatic").to_string(),
            OptionRow::Terminal => self.terminal.clone(),
            OptionRow::Editor if self.editor.is_empty() => t!("Built-in").to_string(),
            OptionRow::Editor => self.editor.clone(),
            OptionRow::RememberDirs => on_off(self.remember_dirs),
            OptionRow::PreviewOnStart => on_off(self.preview_on_start),
            // Each in its own words; Automatic says which it came to.
            OptionRow::Language => match self.language {
                Some(language) => language.label().to_string(),
                None => t!("Automatic ({language})", language = crate::i18n::detect().label()),
            },
            OptionRow::Theme => self.theme.label().to_string(),
            OptionRow::ThemeBackground => if self.theme_background { t!("Theme") } else { t!("Terminal") }.to_string(),
            OptionRow::IconStyle => match self.icon_style {
                IconStyle::NerdFont => "Nerd Font",
                IconStyle::NerdFontMono => "Nerd Font Mono",
                IconStyle::Plain => t!("Plain"),
            }
            .to_string(),
            OptionRow::Clock => match self.clock {
                Clock::Hours24 => t!("24-hour"),
                Clock::Hours12 => t!("12-hour"),
                Clock::Off => t!("Off"),
            }
            .to_string(),
            OptionRow::Cursor => match self.cursor {
                CursorShape::Bar => t!("Bar"),
                CursorShape::Block => t!("Block"),
                CursorShape::Underline => t!("Underline"),
                CursorShape::Classic => t!("Classic"),
            }
            .to_string(),
            OptionRow::CursorBlink => on_off(self.cursor_blink),
            OptionRow::TabWidth => self.tab_width.to_string(),
            OptionRow::LineNumbers => on_off(self.line_numbers),
            // Shown as it is drawn, both ways.
            OptionRow::ScrollBars => {
                let (down, along) = self.scroll_bar.thumbs();
                format!("{} {down}{along}", self.scroll_bar.label())
            }
            OptionRow::HighlightLimit => match self.highlight_limit_kib {
                0 => t!("Off").to_string(),
                kib if kib % 1024 == 0 => format!("{} MiB", kib / 1024),
                kib => format!("{} KiB", kib),
            },
            OptionRow::LargeFile => format!("{} MiB", self.large_file_mib),
            OptionRow::ImageDefault => if self.image_fill { t!("Fill") } else { t!("Fit") }.to_string(),
            OptionRow::ImageBackgrounds => on_off(self.image_backgrounds),
            OptionRow::ImagePrefetch => on_off(self.image_prefetch),
        }
    }

    /// The text a text row holds, for typing into.
    pub fn text(&self, row: OptionRow) -> &str {
        match row {
            OptionRow::Editor => &self.editor,
            _ => &self.terminal,
        }
    }

    pub fn set_text(&mut self, row: OptionRow, text: String) {
        match row {
            OptionRow::Editor => self.editor = text,
            OptionRow::Terminal => self.terminal = text,
            _ => {}
        }
    }

    /// Step a row to its next value, or its previous one. A text row does not
    /// step; it is typed into instead.
    pub fn cycle(&mut self, row: OptionRow, forward: bool) {
        fn step<T: Copy + PartialEq>(values: &[T], current: T, forward: bool) -> T {
            let Some(index) = values.iter().position(|&value| value == current) else {
                return values[0];
            };
            let next = if forward { index + 1 } else { index + values.len() - 1 };
            values[next % values.len()]
        }

        match row {
            OptionRow::ShowHidden => self.show_hidden = !self.show_hidden,
            OptionRow::SortKey => {
                let keys = [SortKey::Name, SortKey::Extension, SortKey::Size, SortKey::Modified];
                self.sort_key = step(&keys, self.sort_key, forward);
            }
            OptionRow::SortDirection => self.sort_descending = !self.sort_descending,
            OptionRow::DirsFirst => self.dirs_first = !self.dirs_first,
            OptionRow::CaseSensitive => self.case_sensitive = !self.case_sensitive,
            OptionRow::ColumnExt => self.column_ext = !self.column_ext,
            OptionRow::ColumnSize => self.column_size = !self.column_size,
            OptionRow::ColumnModified => self.column_modified = !self.column_modified,
            OptionRow::ColumnAttributes => self.column_attributes = !self.column_attributes,
            OptionRow::GitMarks => self.git_marks = !self.git_marks,
            OptionRow::DateFormat => {
                let formats = [DateFormat::Short, DateFormat::Iso, DateFormat::Relative];
                self.date_format = step(&formats, self.date_format, forward);
            }
            OptionRow::SizeUnits => self.size_units = if self.size_units == SizeUnits::Binary { SizeUnits::Decimal } else { SizeUnits::Binary },
            OptionRow::ConfirmDelete => self.confirm_delete = !self.confirm_delete,
            OptionRow::DeleteToTrash => self.delete_to_trash = !self.delete_to_trash,
            OptionRow::ConfirmCopyMove => self.confirm_copy_move = !self.confirm_copy_move,
            OptionRow::OnExisting => {
                let policies = [OnExisting::Ask, OnExisting::Overwrite, OnExisting::KeepBoth, OnExisting::Refuse];
                self.on_existing = step(&policies, self.on_existing, forward);
            }
            OptionRow::RememberDirs => self.remember_dirs = !self.remember_dirs,
            OptionRow::PreviewOnStart => self.preview_on_start = !self.preview_on_start,
            OptionRow::Language => {
                let choices: Vec<Option<crate::i18n::Language>> = std::iter::once(None).chain(crate::i18n::Language::ALL.into_iter().map(Some)).collect();
                self.language = step(&choices, self.language, forward);
            }
            OptionRow::Theme => {
                self.theme = step(&Theme::ALL, self.theme, forward);
            }
            OptionRow::ThemeBackground => {
                self.theme_background = !self.theme_background;
            }
            OptionRow::IconStyle => {
                let styles = [IconStyle::NerdFont, IconStyle::NerdFontMono, IconStyle::Plain];
                self.icon_style = step(&styles, self.icon_style, forward);
            }
            OptionRow::Clock => {
                let clocks = [Clock::Hours24, Clock::Hours12, Clock::Off];
                self.clock = step(&clocks, self.clock, forward);
            }
            OptionRow::Cursor => {
                let shapes = [CursorShape::Bar, CursorShape::Block, CursorShape::Underline, CursorShape::Classic];
                self.cursor = step(&shapes, self.cursor, forward);
            }
            OptionRow::CursorBlink => self.cursor_blink = !self.cursor_blink,
            OptionRow::TabWidth => self.tab_width = step(&TAB_WIDTHS, self.tab_width, forward),
            OptionRow::LineNumbers => self.line_numbers = !self.line_numbers,
            OptionRow::ScrollBars => self.scroll_bar = step(&ScrollBarStyle::ALL, self.scroll_bar, forward),
            OptionRow::HighlightLimit => self.highlight_limit_kib = step(&HIGHLIGHT_LIMITS_KIB, self.highlight_limit_kib, forward),
            OptionRow::LargeFile => self.large_file_mib = step(&LARGE_FILE_LIMITS_MIB, self.large_file_mib, forward),
            OptionRow::ImageDefault => self.image_fill = !self.image_fill,
            OptionRow::ImageBackgrounds => self.image_backgrounds = !self.image_backgrounds,
            OptionRow::ImagePrefetch => self.image_prefetch = !self.image_prefetch,
            OptionRow::Terminal | OptionRow::Editor => {}
        }
    }
}
