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

pub const OPTION_ROWS: [OptionRow; 33] = [
    OptionRow::ShowHidden,
    OptionRow::SortKey,
    OptionRow::SortDirection,
    OptionRow::DirsFirst,
    OptionRow::CaseSensitive,
    OptionRow::ColumnExt,
    OptionRow::ColumnSize,
    OptionRow::ColumnModified,
    OptionRow::ColumnAttributes,
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
            OptionRow::ShowHidden => "Show hidden files",
            OptionRow::SortKey => "Sort by",
            OptionRow::SortDirection => "Sort direction",
            OptionRow::DirsFirst => "Directories first",
            OptionRow::CaseSensitive => "Case-sensitive sort",
            OptionRow::ColumnExt => "Ext column",
            OptionRow::ColumnSize => "Size column",
            OptionRow::ColumnModified => "Modified column",
            OptionRow::ColumnAttributes => "Attributes column",
            OptionRow::DateFormat => "Date format",
            OptionRow::SizeUnits => "Size units",
            OptionRow::ConfirmDelete => "Confirm delete",
            OptionRow::DeleteToTrash => "Delete to trash (F8)",
            OptionRow::ConfirmCopyMove => "Confirm copy and move",
            OptionRow::OnExisting => "When destination exists",
            OptionRow::Terminal => "Terminal (F9)",
            OptionRow::Editor => "Editor (F4)",
            OptionRow::RememberDirs => "Remember directories",
            OptionRow::PreviewOnStart => "Preview on startup",
            OptionRow::Theme => "Theme",
            OptionRow::ThemeBackground => "Background",
            OptionRow::IconStyle => "Icons",
            OptionRow::Clock => "Clock",
            OptionRow::Cursor => "Cursor",
            OptionRow::CursorBlink => "Cursor blinks",
            OptionRow::TabWidth => "Tab width",
            OptionRow::LineNumbers => "Line numbers",
            OptionRow::ScrollBars => "Scroll bars",
            OptionRow::HighlightLimit => "Highlight files up to",
            OptionRow::LargeFile => "Ask before opening over",
            OptionRow::ImageDefault => "Images open as",
            OptionRow::ImageBackgrounds => "Image backgrounds",
            OptionRow::ImagePrefetch => "Read pictures ahead",
        }
    }

    /// The heading this row opens, when it is the first of its group.
    pub fn section(self) -> Option<&'static str> {
        match self {
            OptionRow::ShowHidden => Some("Panels"),
            OptionRow::ConfirmDelete => Some("Behaviour"),
            OptionRow::Theme => Some("Appearance"),
            OptionRow::TabWidth => Some("Viewer and editor"),
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
        matches!(self, OptionRow::ShowHidden | OptionRow::SortKey | OptionRow::SortDirection | OptionRow::DirsFirst | OptionRow::CaseSensitive | OptionRow::SizeUnits)
    }
}

impl Options {
    pub fn value(&self, row: OptionRow) -> String {
        let on_off = |on: bool| if on { "On" } else { "Off" }.to_string();
        match row {
            OptionRow::ShowHidden => on_off(self.show_hidden),
            OptionRow::SortKey => match self.sort_key {
                SortKey::Name => "Name",
                SortKey::Extension => "Extension",
                SortKey::Size => "Size",
                SortKey::Modified => "Modified",
            }
            .to_string(),
            OptionRow::SortDirection => if self.sort_descending { "Descending" } else { "Ascending" }.to_string(),
            OptionRow::DirsFirst => on_off(self.dirs_first),
            OptionRow::CaseSensitive => on_off(self.case_sensitive),
            OptionRow::ColumnExt => on_off(self.column_ext),
            OptionRow::ColumnSize => on_off(self.column_size),
            OptionRow::ColumnModified => on_off(self.column_modified),
            OptionRow::ColumnAttributes => on_off(self.column_attributes),
            OptionRow::DateFormat => match self.date_format {
                DateFormat::Short => "dd/mm/yy",
                DateFormat::Iso => "yyyy-mm-dd",
                DateFormat::Relative => "Relative",
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
                OnExisting::Ask => "Ask",
                OnExisting::Overwrite => "Overwrite",
                OnExisting::Refuse => "Refuse",
                OnExisting::KeepBoth => "Keep both",
            }
            .to_string(),
            OptionRow::Terminal if self.terminal.is_empty() => "Automatic".to_string(),
            OptionRow::Terminal => self.terminal.clone(),
            OptionRow::Editor if self.editor.is_empty() => "Built-in".to_string(),
            OptionRow::Editor => self.editor.clone(),
            OptionRow::RememberDirs => on_off(self.remember_dirs),
            OptionRow::PreviewOnStart => on_off(self.preview_on_start),
            OptionRow::Theme => self.theme.label().to_string(),
            OptionRow::ThemeBackground => if self.theme_background { "Theme" } else { "Terminal" }.to_string(),
            OptionRow::IconStyle => match self.icon_style {
                IconStyle::NerdFont => "Nerd Font",
                IconStyle::NerdFontMono => "Nerd Font Mono",
                IconStyle::Plain => "Plain",
            }
            .to_string(),
            OptionRow::Clock => match self.clock {
                Clock::Hours24 => "24-hour",
                Clock::Hours12 => "12-hour",
                Clock::Off => "Off",
            }
            .to_string(),
            OptionRow::Cursor => match self.cursor {
                CursorShape::Bar => "Bar",
                CursorShape::Block => "Block",
                CursorShape::Underline => "Underline",
                CursorShape::Classic => "Classic",
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
                0 => "Off".to_string(),
                kib if kib % 1024 == 0 => format!("{} MiB", kib / 1024),
                kib => format!("{} KiB", kib),
            },
            OptionRow::LargeFile => format!("{} MiB", self.large_file_mib),
            OptionRow::ImageDefault => if self.image_fill { "Fill" } else { "Fit" }.to_string(),
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
